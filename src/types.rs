use tokio::sync::{mpsc, oneshot};

use crate::audio::{AudioCommand, AudioCommandError, AudioCommandPayload, GlobalVolumeMap};

#[derive(Debug, Clone)]
pub struct Data{
    audiocommand: mpsc::Sender<AudioCommand>,
    _volume_map: GlobalVolumeMap
}

impl Data {
    pub fn new(audiocommand: mpsc::Sender<AudioCommand>, volume_map: GlobalVolumeMap) -> Self{
        Self { audiocommand, _volume_map: volume_map}
    }
    pub async fn command(&self, payload: AudioCommandPayload) -> Result<(), AudioCommandError> {
        let (tx, rx) = oneshot::channel();
        self.audiocommand.send(AudioCommand{payload, tx }).await.or_else(|_| Err(AudioCommandError::ProviderDropped))?;
        rx.await.or_else(|_| Err(AudioCommandError::ProviderDropped))?
    }
}

pub type Ctx<'a> = poise::Context<'a, Data, anyhow::Error>;

