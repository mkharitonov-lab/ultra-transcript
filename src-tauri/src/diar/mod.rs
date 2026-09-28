//! Диаризация — кто когда говорил. Движок выбирается в настройках:
//! pyannote 3.0 (sherpa-onnx), pyannote community-1 (VBx), NVIDIA Nemotron 3 (NeMo-Speech.cpp)
//! или без разделения — тогда вся запись достаётся одному спикеру.

mod community1;
pub mod nemotron;
mod pyannote3;
mod vbx;

use crate::speech::Turn;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub use community1::DIR as COMMUNITY1_DIR;

/// Движок диаризации.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiarModel {
    /// Без разделения: вся запись — один спикер.
    Off,
    /// pyannote 3.0 через sherpa-onnx: иерархическая кластеризация с порогом из настроек.
    #[default]
    Pyannote3,
    /// pyannote community-1: те же нейросети, кластеризация VBx с PLDA.
    Community1,
    /// NVIDIA Nemotron 3 Diarization: одна сеть сразу выдаёт спикеров, до 8 человек.
    Nemotron3,
}

impl DiarModel {
    pub fn title(self) -> &'static str {
        match self {
            Self::Off => crate::lang::tr("без разделения на спикеров", "no speaker separation"),
            Self::Pyannote3 => "pyannote 3.0",
            Self::Community1 => "pyannote community-1",
            Self::Nemotron3 => "Nemotron 3",
        }
    }
}

enum Engine {
    Off,
    Pyannote3(pyannote3::Pyannote3),
    Community1(Box<community1::Community1>),
    Nemotron3(nemotron::Nemotron3),
}

pub struct Diarizer {
    pub model: DiarModel,
    engine: Engine,
}

impl Diarizer {
    pub fn load(models: &Path, model: DiarModel) -> Result<Self> {
        let engine = match model {
            DiarModel::Off => Engine::Off,
            DiarModel::Pyannote3 => Engine::Pyannote3(pyannote3::Pyannote3::load(models)?),
            DiarModel::Community1 => Engine::Community1(Box::new(community1::Community1::load(models)?)),
            DiarModel::Nemotron3 => Engine::Nemotron3(nemotron::Nemotron3::load(models)?),
        };
        Ok(Self { model, engine })
    }

    /// Отрезки речи по спикерам. `threshold` — порог кластеризации pyannote 3.0 (у остальных движков
    /// число спикеров определяется само); `progress` получает долю выполненного.
    pub fn diarize(&mut self, samples: &[f32], threshold: f32, progress: &mut dyn FnMut(f32)) -> Result<Vec<Turn>> {
        match &mut self.engine {
            Engine::Off => Ok(vec![]),
            Engine::Pyannote3(e) => e.diarize(samples, threshold, progress),
            Engine::Community1(e) => e.diarize(samples, progress),
            Engine::Nemotron3(e) => e.diarize(samples, progress),
        }
    }
}
