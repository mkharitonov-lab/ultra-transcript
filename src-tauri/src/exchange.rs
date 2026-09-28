//! Импорт и экспорт справочников «Словарь» и «Кто есть кто» в .xlsx и .csv.
//! Голосовые профили в файлы не попадают: голос загружается отдельно, в карточке человека.

use crate::lang::{is_english, tr};
use crate::store::{Person, Store, Term};
use anyhow::{anyhow, bail, Context, Result};
use calamine::Reader;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Какой справочник.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Directory {
    Terms,
    People,
}

/// Итог импорта: строк с данными в файле, из них новых и обновлённых записей.
#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub total: usize,
    pub added: usize,
    pub updated: usize,
}

/// Столбец файла: заголовок при выгрузке и как его могут назвать в чужой таблице (см. `header_key`).
struct Column {
    title: &'static str,
    /// Заголовок при выгрузке с английским интерфейсом.
    en: &'static str,
    names: &'static [&'static str],
    /// Значения через запятую; перенос строки внутри ячейки — тоже разделитель.
    list: bool,
}

const TERM_COLUMNS: &[Column] = &[
    Column { title: "Термин", en: "Term", names: &["термин", "term", "аббревиатура", "название"], list: false },
    Column {
        title: "Как слышится",
        en: "Sounds like",
        names: &["как слышится", "sounds like", "aliases", "варианты", "варианты написания", "синонимы"],
        list: true,
    },
    Column {
        title: "Определение",
        en: "Definition",
        names: &["определение", "definition", "описание", "пояснение", "расшифровка"],
        list: false,
    },
];

const PEOPLE_COLUMNS: &[Column] = &[
    Column { title: "ФИО", en: "Name", names: &["фио", "ф.и.о", "name", "full name", "человек", "сотрудник"], list: false },
    Column {
        title: "Обращения",
        en: "Also called",
        names: &["обращения", "обращение", "also called", "aliases", "как обращаются"],
        list: true,
    },
    Column { title: "Должность", en: "Role", names: &["должность", "role", "роль", "position"], list: false },
    Column {
        title: "Организация",
        en: "Organization",
        names: &["организация", "org", "organization", "компания", "company"],
        list: false,
    },
];

/// ФИО по отдельным столбцам — как в выгрузках кадровых систем.
const NAME_PARTS: [&str; 3] = ["фамилия", "имя", "отчество"];

impl Directory {
    fn columns(self) -> &'static [Column] {
        match self {
            Self::Terms => TERM_COLUMNS,
            Self::People => PEOPLE_COLUMNS,
        }
    }

    fn sheet(self) -> &'static str {
        match self {
            Self::Terms => tr("Словарь", "Glossary"),
            Self::People => tr("Кто есть кто", "People"),
        }
    }
}

/// Выгружает проверенные записи — находки LLM, ждущие проверки, в файл не попадают.
/// Формат — по расширению файла. Возвращает число записей.
pub fn export(store: &Store, dir: Directory, path: &Path) -> Result<usize> {
    let rows: Vec<Vec<String>> = match dir {
        Directory::Terms => store
            .terms()?
            .into_iter()
            .filter(|t| !t.pending)
            .map(|t| vec![t.term, t.aliases, t.definition])
            .collect(),
        Directory::People => store
            .people()?
            .into_iter()
            .filter(|p| !p.pending)
            .map(|p| vec![p.name, p.aliases, p.role, p.org])
            .collect(),
    };
    let header: Vec<&str> = dir.columns().iter().map(|c| if is_english() { c.en } else { c.title }).collect();
    match extension(path).as_str() {
        "xlsx" => write_xlsx(path, dir.sheet(), &header, &rows),
        "csv" => write_csv(path, &header, &rows),
        _ => bail!(tr("сохраните файл как .xlsx или .csv", "save the file as .xlsx or .csv")),
    }
    .with_context(|| format!("{} {}", tr("не удалось сохранить", "could not save"), path.display()))?;
    Ok(rows.len())
}

/// Загружает записи из таблицы: новые добавляются, совпадающие с уже известными
/// дополняются непустыми ячейками и считаются проверенными.
pub fn import(store: &Store, dir: Directory, path: &Path) -> Result<Report> {
    let recs = records(dir, read_table(path)?)?;
    let (added, updated) = match dir {
        Directory::Terms => store.merge_terms(
            &recs
                .iter()
                .map(|r| Term { term: r[0].clone(), aliases: r[1].clone(), definition: r[2].clone(), ..Default::default() })
                .collect::<Vec<_>>(),
        )?,
        Directory::People => store.merge_people(
            &recs
                .iter()
                .map(|r| Person {
                    name: r[0].clone(),
                    aliases: r[1].clone(),
                    role: r[2].clone(),
                    org: r[3].clone(),
                    ..Default::default()
                })
                .collect::<Vec<_>>(),
        )?,
    };
    Ok(Report { total: recs.len(), added, updated })
}

/// Строки таблицы → записи с полями в порядке столбцов справочника. Первая строка — заголовки
/// в любом порядке; если ни один не узнан, это уже данные, а столбцы — как при выгрузке.
/// Строки без первого поля (термина, ФИО) пропускаются.
fn records(dir: Directory, mut rows: Vec<Vec<String>>) -> Result<Vec<Vec<String>>> {
    let columns = dir.columns();
    rows.retain(|r| r.iter().any(|c| !c.trim().is_empty()));
    let Some(first) = rows.first() else { return Ok(vec![]) };
    let header: Vec<String> = first.iter().map(|h| header_key(h)).collect();
    let find = |names: &[&str]| header.iter().position(|h| names.iter().any(|n| h == n));
    // Номер столбца файла для каждого поля; `parts` — ФИО, собранное из частей.
    let (map, parts): (Vec<Option<usize>>, Vec<Option<usize>>) = if header.iter().any(|h| is_known(h)) {
        let map: Vec<_> = columns.iter().map(|c| find(c.names)).collect();
        let parts: Vec<_> = match (dir, map[0]) {
            (Directory::People, None) => NAME_PARTS.iter().map(|p| find(&[*p])).collect(),
            _ => vec![],
        };
        if map[0].is_none() && parts.iter().all(Option::is_none) {
            let title = |c: &Column| if is_english() { c.en } else { c.title };
            let titles: Vec<&str> = columns.iter().map(title).collect();
            bail!(
                "{} «{}». {}: {}",
                tr("в первой строке нет столбца", "the first row has no column"),
                title(&columns[0]),
                tr("Ожидаются столбцы", "Expected columns"),
                titles.join(", ")
            );
        }
        rows.remove(0);
        (map, parts)
    } else {
        ((0..columns.len()).map(Some).collect(), vec![])
    };
    fn cell(r: &[String], i: Option<usize>) -> &str {
        i.and_then(|i| r.get(i)).map_or("", String::as_str)
    }
    Ok(rows
        .iter()
        .map(|r| {
            let mut rec: Vec<String> = columns.iter().zip(&map).map(|(c, &i)| tidy(cell(r, i), c.list)).collect();
            if !parts.is_empty() {
                let name: Vec<String> = parts.iter().map(|&i| tidy(cell(r, i), false)).filter(|s| !s.is_empty()).collect();
                rec[0] = name.join(" ");
            }
            rec
        })
        .filter(|rec| !rec[0].is_empty())
        .collect())
}

/// Заголовок для сравнения: без регистра, «ё», пояснений в скобках и знаков в конце.
fn header_key(h: &str) -> String {
    let h = h.split('(').next().unwrap_or_default().to_lowercase().replace('ё', "е");
    h.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches([':', '*', '.']).to_string()
}

fn is_known(h: &str) -> bool {
    let known = |names: &[&str]| names.iter().any(|n| *n == h);
    TERM_COLUMNS.iter().chain(PEOPLE_COLUMNS).any(|c| known(c.names)) || known(&NAME_PARTS)
}

/// Ячейка → одна строка без лишних пробелов; в списках переносы строк становятся запятыми.
fn tidy(cell: &str, list: bool) -> String {
    let line = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    if list {
        cell.lines().map(line).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
    } else {
        line(cell)
    }
}

fn extension(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase()
}

fn read_table(path: &Path) -> Result<Vec<Vec<String>>> {
    match extension(path).as_str() {
        "csv" | "tsv" | "txt" => read_csv(path),
        "xlsx" | "xlsm" | "xls" | "ods" => read_sheet(path),
        _ => bail!(tr("поддерживаются таблицы .xlsx, .xls, .ods и .csv", "supported tables are .xlsx, .xls, .ods and .csv")),
    }
    .with_context(|| format!("{} {}", tr("не удалось прочитать", "could not read"), path.display()))
}

/// Первый лист книги Excel или OpenDocument.
fn read_sheet(path: &Path) -> Result<Vec<Vec<String>>> {
    let mut book = calamine::open_workbook_auto(path)?;
    let range = book.worksheet_range_at(0).ok_or_else(|| anyhow!(tr("в книге нет листов", "the workbook has no sheets")))??;
    Ok(range
        .rows()
        .map(|r| {
            r.iter()
                .map(|c| match c {
                    calamine::Data::Error(_) => String::new(),
                    c => c.to_string().trim().to_string(),
                })
                .collect()
        })
        .collect())
}

fn read_csv(path: &Path) -> Result<Vec<Vec<String>>> {
    let text = decode_text(&std::fs::read(path)?);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(detect_delimiter(&text))
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    reader
        .records()
        .map(|r| -> Result<Vec<String>> { Ok(r?.iter().map(|c| c.trim().to_string()).collect()) })
        .collect()
}

/// Текст CSV: UTF-8 (с BOM или без), UTF-16 с BOM, иначе Windows-1251 — так сохраняет CSV русский Excel.
fn decode_text(bytes: &[u8]) -> String {
    if let Some((encoding, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        return encoding.decode_without_bom_handling(&bytes[bom..]).0.into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => encoding_rs::WINDOWS_1251.decode_without_bom_handling(bytes).0.into_owned(),
    }
}

/// Разделитель — тот из «;», «,» и табуляции, что чаще встречается в первой строке вне кавычек.
fn detect_delimiter(text: &str) -> u8 {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default();
    let mut counts = [(b';', 0), (b',', 0), (b'\t', 0)];
    let mut quoted = false;
    for b in line.bytes() {
        if b == b'"' {
            quoted = !quoted;
        } else if let Some(c) = counts.iter_mut().find(|(d, _)| !quoted && *d == b) {
            c.1 += 1;
        }
    }
    // При равенстве побеждает первый по списку: max_by_key берёт последний из равных.
    counts.iter().rev().max_by_key(|(_, n)| *n).filter(|(_, n)| *n > 0).map_or(b';', |(d, _)| *d)
}

/// CSV для Excel: UTF-8 с BOM, иначе кириллица превратится в кракозябры, и «;» — разделитель
/// списков в русской локали, иначе все поля окажутся в одном столбце.
fn write_csv(path: &Path, header: &[&str], rows: &[Vec<String>]) -> Result<()> {
    let mut out = "\u{feff}".as_bytes().to_vec();
    {
        let mut w = csv::WriterBuilder::new().delimiter(b';').terminator(csv::Terminator::CRLF).from_writer(&mut out);
        w.write_record(header)?;
        for r in rows {
            w.write_record(r)?;
        }
        w.flush()?;
    }
    std::fs::write(path, out)?;
    Ok(())
}

fn write_xlsx(path: &Path, sheet: &str, header: &[&str], rows: &[Vec<String>]) -> Result<()> {
    use rust_xlsxwriter::{Format, Workbook};
    let mut book = Workbook::new();
    let ws = book.add_worksheet();
    ws.set_name(sheet)?;
    let bold = Format::new().set_bold();
    for (c, h) in header.iter().enumerate() {
        ws.write_string_with_format(0, c as u16, *h, &bold)?;
    }
    // Всё — строками: иначе Excel превратит «1С» или номера телефонов в числа.
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate().filter(|(_, v)| !v.is_empty()) {
            ws.write_string(r as u32 + 1, c as u16, v)?;
        }
    }
    ws.set_freeze_panes(1, 0)?;
    ws.autofilter(0, 0, rows.len() as u32, header.len() as u16 - 1)?;
    ws.set_autofit_max_width(420).autofit();
    book.save(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(lines: &[&[&str]]) -> Vec<Vec<String>> {
        lines.iter().map(|l| l.iter().map(|s| s.to_string()).collect()).collect()
    }

    #[test]
    fn columns_by_header_in_any_order() {
        let got = records(
            Directory::Terms,
            rows(&[
                &["Определение", "ТЕРМИН:", "Варианты (как слышится)"],
                &["Министерство", " Минпромторг ", "минпром торг\nмин промторг"],
                &["без термина", "", ""],
                &[],
            ]),
        )
        .unwrap();
        assert_eq!(got, rows(&[&["Минпромторг", "минпром торг, мин промторг", "Министерство"]]));
    }

    #[test]
    fn without_header_columns_go_in_export_order() {
        let got = records(Directory::People, rows(&[&["Петров Сергей Иванович", "Сергей Иванович", "Директор"]])).unwrap();
        assert_eq!(got, rows(&[&["Петров Сергей Иванович", "Сергей Иванович", "Директор", ""]]));
    }

    #[test]
    fn name_from_separate_columns() {
        let got = records(
            Directory::People,
            rows(&[&["Фамилия", "Имя", "Отчество", "Должность"], &["Петров", "Сергей", "Иванович", "Директор"]]),
        )
        .unwrap();
        assert_eq!(got, rows(&[&["Петров Сергей Иванович", "", "Директор", ""]]));
    }

    #[test]
    fn wrong_directory_file_is_rejected() {
        let people = rows(&[&["ФИО", "Должность"], &["Петров", "Директор"]]);
        let err = records(Directory::Terms, people).unwrap_err().to_string();
        assert!(err.contains("нет столбца «Термин»"), "{err}");
    }

    #[test]
    fn csv_delimiter_and_encoding() {
        assert_eq!(detect_delimiter("Термин;Как слышится;Определение\r\nа;б, в;г"), b';');
        assert_eq!(detect_delimiter("Термин,\"минпром торг; мин промторг\",Определение"), b',');
        assert_eq!(detect_delimiter("ФИО\tДолжность"), b'\t');
        assert_eq!(detect_delimiter("Минпромторг"), b';');
        let (cp1251, _, _) = encoding_rs::WINDOWS_1251.encode("Термин;Определение");
        assert_eq!(decode_text(&cp1251), "Термин;Определение");
        assert_eq!(decode_text("\u{feff}Термин".as_bytes()), "Термин");
    }

    #[test]
    fn round_trip_through_files() {
        let dir = std::env::temp_dir().join(format!("ut-exchange-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let header = ["Термин", "Как слышится", "Определение"];
        let data = rows(&[&["1С", "один эс", "Платформа; \"учётная\""], &["ГИСП", "", "Система"]]);
        let mut expected = rows(&[&header]);
        expected.extend(data.clone());
        for name in ["словарь.csv", "словарь.xlsx"] {
            let path = dir.join(name);
            match extension(&path).as_str() {
                "csv" => write_csv(&path, &header, &data).unwrap(),
                _ => write_xlsx(&path, "Словарь", &header, &data).unwrap(),
            }
            let mut got = read_table(&path).unwrap();
            got.iter_mut().for_each(|r| r.resize(3, String::new()));
            assert_eq!(got, expected, "{name}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
