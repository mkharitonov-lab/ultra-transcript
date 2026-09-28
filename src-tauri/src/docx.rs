//! Движок .docx-шаблонов. Шаблон — обычный документ Word с метками:
//!   {{поле}}              — подставляется значение;
//!   {{список}}            — в абзаце-пункте списка: абзац повторяется для каждого элемента;
//!   {{таблица.колонка}}   — строка таблицы (или абзац) повторяется для каждого элемента.
//! Пользователь правит шаблон в Word; набор меток шаблона и есть схема данных.

use anyhow::{Context, Result};
use regex::{Captures, Regex};
use serde_json::{Map, Value};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::LazyLock;

static PARA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<w:p(?:\s[^>]*)?/>|<w:p(?:\s[^>]*)?>.*?</w:p>").unwrap());
static ROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<w:tr(?:\s[^>]*)?>.*?</w:tr>").unwrap());
static TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)(<w:t(?:\s[^>]*)?>)(.*?)(</w:t>)").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{\s*([^{}]+?)\s*\}\}").unwrap());

#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Text(String),
    List(String),
    Table(String, Vec<String>),
}

/// Метки шаблона → схема полей (для LLM).
pub fn fields(template: &Path) -> Result<Vec<Field>> {
    let xml = heal(&read_part(template, "word/document.xml")?);
    let mut out: Vec<Field> = vec![];
    for p in PARA.find_iter(&xml) {
        let p = p.as_str();
        let numbered = p.contains("<w:numPr>");
        for c in TAG.captures_iter(&plain_text(p)) {
            let tag = c[1].to_string();
            let field = match tag.split_once('.') {
                Some((t, col)) => {
                    if let Some(Field::Table(_, cols)) =
                        out.iter_mut().find(|f| matches!(f, Field::Table(n, _) if n == t))
                    {
                        if !cols.contains(&col.to_string()) {
                            cols.push(col.to_string());
                        }
                        continue;
                    }
                    Field::Table(t.to_string(), vec![col.to_string()])
                }
                None if numbered => Field::List(tag),
                None => Field::Text(tag),
            };
            if !out.iter().any(|f| name(f) == name(&field)) {
                out.push(field);
            }
        }
    }
    Ok(out)
}

pub fn name(f: &Field) -> &str {
    match f {
        Field::Text(n) | Field::List(n) | Field::Table(n, _) => n,
    }
}

/// Заполняет шаблон данными и сохраняет результат.
pub fn render(template: &Path, data: &Map<String, Value>, output: &Path) -> Result<()> {
    let mut zin = zip::ZipArchive::new(std::fs::File::open(template).context("шаблон не найден")?)?;
    if let Some(dir) = output.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = output.with_extension("docx.tmp");
    let mut zout = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
    for i in 0..zin.len() {
        let mut f = zin.by_index(i)?;
        let fname = f.name().to_string();
        let mut buf = vec![];
        f.read_to_end(&mut buf)?;
        let is_body = fname == "word/document.xml"
            || (fname.starts_with("word/header") || fname.starts_with("word/footer")) && fname.ends_with(".xml");
        if is_body {
            buf = fill(&String::from_utf8(buf)?, data).into_bytes();
        }
        zout.start_file(fname, zip::write::SimpleFileOptions::default())?;
        zout.write_all(&buf)?;
    }
    zout.finish()?;
    std::fs::rename(tmp, output)?;
    Ok(())
}

fn fill(xml: &str, data: &Map<String, Value>) -> String {
    let xml = heal(xml);
    // Сначала размножаем строки таблиц, затем абзацы, затем подставляем значения.
    let xml = ROW.replace_all(&xml, |c: &Captures| repeat(&c[0], data)).into_owned();
    let xml = PARA.replace_all(&xml, |c: &Captures| repeat(&c[0], data)).into_owned();
    TAG.replace_all(&xml, |c: &Captures| escape(&scalar(data.get(c[1].trim())))).into_owned()
}

/// Если в блоке есть метка списка — повторить блок для каждого элемента.
fn repeat(block: &str, data: &Map<String, Value>) -> String {
    let list = TAG.captures_iter(block).find_map(|c| {
        let key = c[1].split('.').next().unwrap().trim().to_string();
        match data.get(&key) {
            Some(Value::Array(items)) => Some((key, items)),
            _ => None,
        }
    });
    let Some((key, items)) = list else { return block.to_string() };
    items
        .iter()
        .map(|item| {
            TAG.replace_all(block, |c: &Captures| {
                let tag = c[1].trim();
                if tag == key {
                    escape(&scalar(Some(item)))
                } else if let Some(col) = tag.strip_prefix(&format!("{key}.")) {
                    escape(&scalar(item.get(col)))
                } else {
                    c[0].to_string()
                }
            })
            .into_owned()
        })
        .collect()
}

fn scalar(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.iter().map(|x| scalar(Some(x))).collect::<Vec<_>>().join(", "),
        Some(Value::Object(o)) => o.values().map(|x| scalar(Some(x))).collect::<Vec<_>>().join(" — "),
        Some(v) => v.to_string(),
    }
}

/// XML-экранирование; переводы строк — разрывы строки Word.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\n', "</w:t><w:br/><w:t xml:space=\"preserve\">")
}

/// Word разбивает «{{метку}}» на несколько фрагментов текста — склеиваем их обратно,
/// сохраняя форматирование первого фрагмента.
fn heal(xml: &str) -> String {
    PARA.replace_all(xml, |c: &Captures| {
        let p = &c[0];
        if !plain_text(p).contains("{{") {
            return p.to_string();
        }
        let mut texts: Vec<String> = TEXT.captures_iter(p).map(|t| t[2].to_string()).collect();
        for i in 0..texts.len() {
            let mut j = i + 1;
            while j < texts.len() && unclosed(&texts[i], &texts[j]) {
                let next = std::mem::take(&mut texts[j]);
                match next.find("}}") {
                    Some(k) if texts[i].contains("{{") => {
                        texts[i].push_str(&next[..k + 2]);
                        texts[j] = next[k + 2..].to_string();
                    }
                    _ => {
                        texts[i].push_str(&next);
                        j += 1;
                    }
                }
            }
        }
        let mut n = 0;
        TEXT.replace_all(p, |t: &Captures| {
            let s = format!("<w:t xml:space=\"preserve\">{}{}", texts[n], &t[3]);
            n += 1;
            s
        })
        .into_owned()
    })
    .into_owned()
}

fn unclosed(cur: &str, next: &str) -> bool {
    let open = cur.rfind("{{").map(|i| i as i64).unwrap_or(-1);
    let close = cur.rfind("}}").map(|i| i as i64).unwrap_or(-1);
    open > close || (cur.ends_with('{') && !cur.ends_with("}{") && next.starts_with('{'))
}

fn plain_text(p: &str) -> String {
    TEXT.captures_iter(p).map(|t| t[2].to_string()).collect()
}

fn read_part(docx: &Path, part: &str) -> Result<String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(docx).context("шаблон не найден")?)?;
    let mut s = String::new();
    z.by_name(part)?.read_to_string(&mut s)?;
    Ok(s)
}

// ---------- шаблоны по умолчанию (создаются при первом запуске, дальше их правит пользователь) ----------

fn para(text: &str, ppr: &str, rpr: &str) -> String {
    format!(r#"<w:p><w:pPr>{ppr}</w:pPr><w:r><w:rPr>{rpr}</w:rPr><w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#)
}

fn run(text: &str, rpr: &str) -> String {
    format!(r#"<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space="preserve">{text}</w:t></w:r>"#)
}

const BOLD: &str = "<w:b/>";
const GREY: &str = r#"<w:color w:val="808080"/><w:sz w:val="18"/>"#;
const CENTER: &str = r#"<w:jc w:val="center"/>"#;
const BULLET: &str = r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>"#;
const NUMBERED: &str = r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr>"#;
const H: &str = r#"<w:pStyle w:val="Heading2"/>"#;

fn cell(content: &str, width: u32, header: bool) -> String {
    let shade = if header { r#"<w:shd w:val="clear" w:color="auto" w:fill="EDEDED"/>"# } else { "" };
    format!(r#"<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa"/>{shade}</w:tcPr>{content}</w:tc>"#)
}

pub fn default_protocol() -> String {
    let widths = [5200, 2400, 1800];
    let head: String = ["Поручение", "Ответственный", "Срок"]
        .iter()
        .zip(widths)
        .map(|(t, w)| cell(&para(t, "", BOLD), w, true))
        .collect();
    let row: String = ["{{поручения.что}}", "{{поручения.ответственный}}", "{{поручения.срок}}"]
        .iter()
        .zip(widths)
        .map(|(t, w)| cell(&para(t, "", ""), w, false))
        .collect();
    let borders = ["top", "left", "bottom", "right", "insideH", "insideV"]
        .iter()
        .map(|b| format!(r#"<w:{b} w:val="single" w:sz="4" w:color="A0A0A0"/>"#))
        .collect::<String>();
    [
        para("ПРОТОКОЛ СОВЕЩАНИЯ", CENTER, r#"<w:b/><w:sz w:val="28"/>"#),
        para("{{тема}}", CENTER, ""),
        para("", "", ""),
        format!("<w:p>{}{}</w:p>", run("Дата: ", BOLD), run("{{дата}}", "")),
        format!("<w:p>{}{}</w:p>", run("Участники: ", BOLD), run("{{участники}}", "")),
        para("Повестка", H, ""),
        para("{{повестка}}", BULLET, ""),
        para("Кратко о ходе обсуждения", H, ""),
        para("{{краткое_содержание}}", "", ""),
        para("Решения", H, ""),
        para("{{решения}}", NUMBERED, ""),
        para("Поручения", H, ""),
        format!(
            r#"<w:tbl><w:tblPr><w:tblW w:w="9400" w:type="dxa"/><w:tblBorders>{borders}</w:tblBorders></w:tblPr><w:tblGrid>{}</w:tblGrid><w:tr>{head}</w:tr><w:tr>{row}</w:tr></w:tbl>"#,
            widths.iter().map(|w| format!(r#"<w:gridCol w:w="{w}"/>"#)).collect::<String>()
        ),
        para("Открытые вопросы", H, ""),
        para("{{открытые_вопросы}}", BULLET, ""),
    ]
    .concat()
}

pub fn default_transcript() -> String {
    [
        para("{{название}}", "", r#"<w:b/><w:sz w:val="32"/>"#),
        para("{{дата}} · {{длительность}}", "", GREY),
        format!("<w:p>{}{}</w:p>", run("Участники: ", BOLD), run("{{участники}}", "")),
        para("", "", ""),
        format!(
            r#"<w:p><w:pPr><w:spacing w:after="160"/></w:pPr>{}{}<w:r><w:br/></w:r>{}</w:p>"#,
            run("{{реплики.спикер}}", BOLD),
            run("  {{реплики.время}}", GREY),
            run("{{реплики.текст}}", "")
        ),
    ]
    .concat()
}

/// Собирает минимальный валидный .docx с заданным телом.
pub fn write_docx(path: &Path, body: &str) -> Result<()> {
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let parts: [(&str, String); 6] = [
        ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#.into()),
        ("_rels/.rels", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{R}/officeDocument" Target="word/document.xml"/></Relationships>"#)),
        ("word/_rels/document.xml.rels", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{R}/styles" Target="styles.xml"/><Relationship Id="rId2" Type="{R}/numbering" Target="numbering.xml"/></Relationships>"#)),
        ("word/styles.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:cs="Arial" w:eastAsia="Arial"/><w:sz w:val="22"/><w:lang w:val="ru-RU"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="80" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="80"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="24"/></w:rPr></w:style></w:styles>"#)),
        ("word/numbering.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#)),
        ("word/document.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1134" w:right="850" w:bottom="1134" w:left="1418" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#)),
    ];
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut z = zip::ZipWriter::new(std::fs::File::create(path)?);
    for (name, content) in parts {
        z.start_file(name, zip::write::SimpleFileOptions::default())?;
        z.write_all(content.as_bytes())?;
    }
    z.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn heals_split_tags_and_fills() {
        let xml = r#"<w:p><w:r><w:t>Дата: {{да</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>та}}</w:t></w:r></w:p><w:p><w:pPr><w:numPr/></w:pPr><w:r><w:t>{{решения}}</w:t></w:r></w:p>"#;
        let data = json!({"дата": "1 мая", "решения": ["А", "Б & В"]});
        let out = fill(xml, data.as_object().unwrap());
        assert!(out.contains("Дата: 1 мая"));
        assert_eq!(out.matches("<w:p>").count(), 3);
        assert!(out.contains("Б &amp; В"));
    }

    #[test]
    fn renders_default_protocol() {
        let dir = std::env::temp_dir().join("ut-docx-test");
        let (tpl, out) = (dir.join("tpl.docx"), dir.join("protocol.docx"));
        write_docx(&tpl, &default_protocol()).unwrap();
        let data = json!({
            "тема": "Цифровой двойник порта", "дата": "28.09.2026", "участники": "Петров С. И., Смирнова А.",
            "повестка": ["Заявка в Минпромторг", "Смета"],
            "краткое_содержание": "Обсудили сроки подачи заявки.",
            "решения": ["Направить заявку до 10 октября", "Согласовать смету на следующей неделе"],
            "поручения": [{"что": "Подготовить заявку", "ответственный": "Петров С. И.", "срок": "10.10.2026"},
                          {"что": "Рассчитать смету", "ответственный": "Смирнова А.", "срок": "05.10.2026"}],
            "открытые_вопросы": ["Финансирование"]
        });
        render(&tpl, data.as_object().unwrap(), &out).unwrap();
        let xml = read_part(&out, "word/document.xml").unwrap();
        assert!(!xml.contains("{{"));
        assert_eq!(xml.matches("Подготовить заявку").count(), 1);
        assert_eq!(xml.matches("<w:tr>").count(), 3);
    }

    #[test]
    fn extracts_fields_from_default_template() {
        let dir = std::env::temp_dir().join("ut-docx-test");
        let path = dir.join("p.docx");
        write_docx(&path, &default_protocol()).unwrap();
        let f = fields(&path).unwrap();
        assert!(f.contains(&Field::List("решения".into())));
        assert!(f.contains(&Field::Text("тема".into())));
        assert!(f.contains(&Field::Table(
            "поручения".into(),
            vec!["что".into(), "ответственный".into(), "срок".into()]
        )));
    }
}
