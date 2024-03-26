use std::collections::HashSet;
use std::num::NonZeroI16;
use std::sync::{Arc, Weak};

use dashmap::DashMap;
use serenity::all::Cache;
use serenity::async_trait;
use serenity::model::id::{ChannelId, GuildId};
use serenity_voice_model::id::UserId;
use serenity_voice_model::payload::ClientDisconnect;
use songbird::input::core::io::MediaSource;
use songbird::input::core::probe::Hint;
use songbird::input::{AudioStream, Input, LiveInput};
use songbird::model::payload::Speaking;
use songbird::tracks::TrackHandle;
use songbird::{Call, CoreEvent, Event, EventContext, EventHandler, Songbird};
use thiserror::Error;
use tokio::runtime;
use tokio::sync::oneshot::Sender;
use tokio::sync::{broadcast, mpsc, Mutex};

#[derive(Debug, Clone)]
pub struct VoiceEventHandler {
    ssrc_map: Arc<Mutex<Vec<(u32, UserId)>>>,
    call: Weak<Mutex<Call>>,
    volume_map: VolumeMap,
    speakers: Arc<Mutex<HashSet<u32>>>,
    txs: Arc<Mutex<AudioTx>>,
}

impl VoiceEventHandler {
    fn new(
        ssrc_map: Arc<Mutex<Vec<(u32, UserId)>>>,
        call: Weak<Mutex<Call>>,
        volume_map: VolumeMap,
        txs: Arc<Mutex<AudioTx>>,
    ) -> Self {
        Self {
            ssrc_map,
            call,
            volume_map,
            txs,
            speakers: Default::default(),
        }
    }
}

pub type VolumeMap = Arc<DashMap<UserId, NonZeroI16>>;
pub type GlobalVolumeMap = Arc<DashMap<GuildId, VolumeMap>>;

#[async_trait]
impl EventHandler for VoiceEventHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            // update users ssrc
            EventContext::SpeakingStateUpdate(Speaking {
                // speaking,
                ssrc,
                user_id,
                ..
            }) => {
                if let Some(uid) = user_id {
                    self.ssrc_map.lock().await.push((*ssrc, uid.clone()))
                }
            }
            // remove users ssrc
            EventContext::ClientDisconnect(ClientDisconnect { user_id, .. }) => {
                let mut map = self.ssrc_map.lock().await;
                let map = &mut *map;
                for i in (0..map.len()).rev() {
                    if &map[i].1 == user_id {
                        map.remove(i);
                    }
                }
            }
            EventContext::VoiceTick(track) => {
                let mut tx = self.txs.lock().await;
                {
                    let now_ssrcs: HashSet<u32> = track.speaking.keys().copied().collect();
                    let mut old_ssrcs = self.speakers.lock().await;
                    for new_ssrc in now_ssrcs.difference(&*old_ssrcs) {
                        tx.new_speaking_ssrc(*new_ssrc).await;
                    }
                    for lost_ssrc in old_ssrcs.difference(&now_ssrcs) {
                        tx.delete_speaking_ssrc(*lost_ssrc);
                    }
                    *old_ssrcs = now_ssrcs;
                }

                for (&ssrc, data) in track.speaking.iter() {
                    if let Some(ref data) = data.decoded_voice {
                        let vol = {
                            if let Some(uid) = self
                                .ssrc_map
                                .lock()
                                .await
                                .iter()
                                .filter(|d| d.0 == ssrc)
                                .map(|d| d.1)
                                .next()
                            {
                                self.volume_map
                                    .get(&uid)
                                    .map(|x| *x)
                                    .map(Into::into)
                                    .unwrap_or(1i16)
                            } else {
                                1
                            }
                        };
                        let bytes = data.iter().step_by(2).flat_map(|x| {
                            let t = x / vol;
                            if cfg!(target_endian = "big") {
                                t.to_be_bytes()
                            } else {
                                t.to_le_bytes()
                            }
                        });
                        tx.send(bytes, ssrc);
                    }
                }
            }
            // EventContext::RtcpPacket(data) => {}
            // EventContext::RtpPacket(packet) => {}
            EventContext::DriverDisconnect(_) => {
                if let Some(c) = self.call.upgrade() {
                    c.lock().await.remove_all_global_events();
                }
            }
            _ => (),
        }
        None
    }
}

pub struct AudioServiceProvider {
    command_rx: mpsc::Receiver<AudioCommand>,
    handler: Arc<AudioServiceHandler>,
}

struct AudioServiceHandler {
    songbirds: Arc<[Arc<Songbird>]>,
    cache: Arc<Cache>,
    volume_map: GlobalVolumeMap,
    txs: Box<[Mutex<Option<Arc<Mutex<AudioTx>>>>]>,
}

pub enum AudioCommandPayload {
    Join(GuildId, ChannelId),
    Remove(GuildId, ChannelId),
    Connect {
        gid: GuildId,
        from_id: ChannelId,
        to_id: ChannelId,
    },
    Disconnect {
        gid: GuildId,
        from_id: ChannelId,
        to_id: ChannelId,
    },
}
pub struct AudioCommand {
    pub payload: AudioCommandPayload,
    pub tx: Sender<Result<(), AudioCommandError>>,
}

#[derive(Error, Debug)]
pub enum AudioCommandError {
    #[error("AudioTx not found")]
    AudioTxNotFound,
    #[error("Channel not found")]
    ChannelNotFound,
    #[error("All bots joined to channel")]
    BotUsedFull,
    #[error("AudioServiceProvider doropped")]
    ProviderDropped,
    #[error("Unknown")]
    UnknownError,
}

impl AudioServiceProvider {
    #[must_use]
    pub fn new(
        songbirds: Arc<[Arc<Songbird>]>,
        command_rx: mpsc::Receiver<AudioCommand>,
        cache: Arc<Cache>,
        volume_map: GlobalVolumeMap,
    ) -> Self {
        AudioServiceProvider {
            command_rx,
            handler: Arc::new(AudioServiceHandler::new(songbirds, cache, volume_map)),
        }
    }
    pub fn run(mut self) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn(async move {
            while let Some(com) = self.command_rx.recv().await {
                let handler = Arc::clone(&self.handler);
                let _ = tokio::task::spawn(async move { handler.handle_command(com).await });
            }
        })
    }
}
impl AudioServiceHandler {
    pub fn new(
        songbirds: Arc<[Arc<Songbird>]>,
        cache: Arc<Cache>,
        volume_map: GlobalVolumeMap,
    ) -> Self {
        Self {
            cache,
            volume_map,
            txs: (0..songbirds.len()).map(|_| None).map(Mutex::new).collect(),
            songbirds,
        }
    }

    pub async fn handle_command(&self, AudioCommand { payload, tx }: AudioCommand) {
        use AudioCommandPayload::*;
        match payload {
            Join(gid, cid) => {
                let _ = tx.send(self.join(gid, cid).await);
            }
            Remove(gid, cid) => {
                let _ = tx.send(self.remove(gid, cid).await);
            }
            Connect {
                gid,
                from_id,
                to_id,
            } => {
                let _ = tx.send(self.connect(gid, from_id, to_id).await);
            }
            Disconnect {
                gid,
                from_id,
                to_id,
            } => {
                let _ = tx.send(self.disconnect(gid, from_id, to_id).await);
            }
        }
    }
    async fn join(&self, gid: GuildId, cid: ChannelId) -> Result<(), AudioCommandError> {
        let Some((idx, unconnected)) = self
            .songbirds
            .iter()
            .enumerate()
            .skip_while(|(_, s)| s.get(gid).is_some())
            .next()
        else {
            return Err(AudioCommandError::BotUsedFull);
        };
        let Ok(_handler) = unconnected.join(gid, cid).await else {
            return Err(AudioCommandError::UnknownError);
        };
        let txs = AudioTx::mutex(5, Arc::clone(&self.songbirds), cid, Arc::clone(&self.cache));
        let volume_map;
        let vm_is_none;
        {
            let vm = &self.volume_map.get(&gid);
            volume_map = match vm {
                Some(volume_map) => Arc::clone(volume_map),
                None => Default::default(),
            };
            vm_is_none = vm.is_none();
        }
        if vm_is_none {
            self.volume_map.insert(gid, Arc::clone(&volume_map));
        }
        let event_handler = VoiceEventHandler::new(
            Default::default(),
            Arc::downgrade(&_handler),
            volume_map,
            Arc::clone(&txs),
        );
        let events = [
            CoreEvent::SpeakingStateUpdate,
            CoreEvent::ClientDisconnect,
            CoreEvent::VoiceTick,
            CoreEvent::RtcpPacket,
            CoreEvent::RtpPacket,
            CoreEvent::DriverDisconnect,
        ];
        let mut handler_lock = _handler.lock().await;
        for e in events {
            handler_lock.add_global_event(Event::Core(e), event_handler.clone())
        }

        let mut x = self.txs[idx].lock().await;
        *x = Some(txs);
        Ok(())
    }
    #[tracing::instrument(skip(self))]
    async fn remove(&self, gid: GuildId, cid: ChannelId) -> Result<(), AudioCommandError> {
        for s in self.songbirds.iter() {
            if s.get(gid).is_some() {
                if let Err(e) = s.remove(gid).await {
                    tracing::warn!("call remove error: {}", e);
                };
            }
        }
        Ok(())
    }
    async fn connect(
        &self,
        gid: GuildId,
        from_id: ChannelId,
        to_id: ChannelId,
    ) -> Result<(), AudioCommandError> {
        let to_idx = get_songbird_index_by_channel_id(&self.songbirds, gid, to_id).await;
        let from_idx = get_songbird_index_by_channel_id(&self.songbirds, gid, from_id).await;
        match (from_idx, to_idx) {
            (Some(from), Some(to)) if from != to => {
                let tx = self.txs[from].lock().await;
                if let Some(tx) = tx.as_ref() {
                    tx.lock().await.connect_to(to);
                    Ok(())
                } else {
                    Err(AudioCommandError::AudioTxNotFound)
                }
            }
            _ => return Err(AudioCommandError::ChannelNotFound),
        }
    }
    async fn disconnect(
        &self,
        gid: GuildId,
        from_id: ChannelId,
        to_id: ChannelId,
    ) -> Result<(), AudioCommandError> {
        let to_idx = get_songbird_index_by_channel_id(&self.songbirds, gid, to_id).await;
        let from_idx = get_songbird_index_by_channel_id(&self.songbirds, gid, from_id).await;
        match (from_idx, to_idx) {
            (Some(from), Some(to)) if from != to => {
                let tx = self.txs[from].lock().await;
                if let Some(tx) = tx.as_ref() {
                    tx.lock().await.disconnect_to(to);
                    Ok(())
                } else {
                    Err(AudioCommandError::AudioTxNotFound)
                }
            }
            _ => return Err(AudioCommandError::ChannelNotFound),
        }
    }
}

async fn get_songbird_index_by_channel_id(
    songbirds: &[Arc<Songbird>],
    gid: GuildId,
    cid: ChannelId,
) -> Option<usize> {
    for (idx, s) in songbirds.iter().enumerate() {
        if let Some(c) = s.get(gid) {
            if c.lock().await.current_channel() == Some(cid.into()) {
                return Some(idx);
            }
        }
    }
    None
}

#[derive(Debug)]
pub struct AutoStopTrackHandle(pub TrackHandle);

impl Drop for AutoStopTrackHandle {
    fn drop(&mut self) {
        eprintln!("track stoped");
        let _ = self.0.stop();
    }
}

#[derive(Debug)]
struct AudioTx {
    txs: Vec<(u32, broadcast::Sender<Arc<[u8]>>)>,
    /// if conntracs[] is Some then it's a voice connection.
    reception_tracks: Box<[Option<Vec<(u32, AutoStopTrackHandle)>>]>,
    songbirds: Arc<[Arc<Songbird>]>,
    buf_size: usize,
    channel_id: ChannelId,
    cache: Arc<Cache>,
}

impl AudioTx {
    pub fn new(
        buf_size: usize,
        driver: Arc<[Arc<Songbird>]>,
        channel_id: ChannelId,
        cache: Arc<Cache>,
    ) -> Self {
        Self {
            txs: Default::default(),
            reception_tracks: (0..driver.len()).map(|_| None).collect(),
            songbirds: driver,
            buf_size,
            channel_id,
            cache,
        }
    }

    pub fn mutex(
        buf_size: usize,
        driver: Arc<[Arc<Songbird>]>,
        channel_id: ChannelId,
        cache: Arc<Cache>,
    ) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self::new(buf_size, driver, channel_id, cache)))
    }

    pub fn connect_to(&mut self, connect_to: usize) {
        self.reception_tracks[connect_to] = Some(Default::default());
        dbg!(self);
    }

    pub fn disconnect_to(&mut self, disconnect_to: usize) {
        self.reception_tracks[disconnect_to] = None;
    }

    pub async fn new_speaking_ssrc(&mut self, ssrc: u32) {
        let guild_id;
        let bit_rate;
        {
            let guild = self.cache.channel(self.channel_id).expect("no guild");
            guild_id = guild.guild_id;
            bit_rate = guild.bitrate.unwrap_or(64 * 1000);
        }
        let mut calls = Vec::new();
        for s in self.songbirds.iter() {
            calls.push(s.get(guild_id));
        }

        let (tx, _) = broadcast::channel(self.buf_size);
        eprintln!("bitrate:{bit_rate}");
        for (index, handle_map) in self
            .reception_tracks
            .iter_mut()
            .enumerate()
            .filter_map(|(i, x)| x.as_mut().map(|h| (i, h)))
        {
            eprintln!("add track: {index}");
            if let Some(Some(call)) = calls.get(index) {
                let track = call
                    .lock()
                    .await
                    .play_input(AudioRx::new_input(&tx, bit_rate as _));
                handle_map.push((ssrc, AutoStopTrackHandle(track)));
            }
        }
        self.txs.push((ssrc, tx));
    }
    pub fn delete_speaking_ssrc(&mut self, ssrc: u32) {
        self.reception_tracks.iter_mut().for_each(|rt| {
            if let Some(ssrc_map) = rt {
                ssrc_map
                    .iter()
                    .position(|(x, _)| *x == ssrc)
                    .map(|pos| ssrc_map.remove(pos));
            }
            self.txs
                .iter()
                .position(|(x, _)| *x == ssrc)
                .map(|pos| self.txs.remove(pos));
        });
    }

    pub fn send(&self, voice: impl IntoIterator<Item = u8>, ssrc: u32) {
        if let Some((_, tx)) = self.txs.iter().find(|(x, _)| *x == ssrc) {
            if tx.receiver_count() == 0 {
                return;
            }
            let value = voice.into_iter().collect();
            let _ = tx.send(value);
        }
    }
}

#[derive(Debug)]
struct AudioRx {
    rx: broadcast::Receiver<Arc<[u8]>>,
    buf: Arc<[u8]>,
    cur: usize,
    handle: runtime::Handle,
}
impl AudioRx {
    pub fn new(tx: &broadcast::Sender<Arc<[u8]>>) -> Self {
        let rx = tx.subscribe();
        Self {
            rx,
            buf: [].as_slice().into(),
            cur: 0,
            handle: runtime::Handle::current(),
        }
    }

    pub fn new_input(tx: &broadcast::Sender<Arc<[u8]>>, _sample_rate: u32) -> Input {
        let input = Box::new(Self::new(tx));
        let mut hint = Hint::new();
        hint.mime_type("audio/wav");
        hint.with_extension("wav");

        Input::Live(
            LiveInput::Raw(AudioStream {
                input,
                hint: Some(hint),
            }),
            None,
        )
    }
}

impl std::io::Read for AudioRx {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // if buf.len() % 2 == 1 {
        //     dbg!(buf.len());
        //     dbg!("invalide alignment");
        //     return Err(Error::new(ErrorKind::InvalidData, "invalide alignment"));
        // }
        let handle = self.handle.clone();
        let mut count = 0;
        for dst in buf.iter_mut() {
            if self.cur == self.buf.len() {
                use broadcast::error::RecvError::*;
                use broadcast::error::TryRecvError;
                self.buf = loop {
                    match self.rx.try_recv() {
                        Ok(d) => break d,
                        Err(TryRecvError::Closed) => return Ok(count),
                        Err(TryRecvError::Lagged(x)) => eprintln!("lagged {x}"),
                        Err(TryRecvError::Empty) => {
                            break loop {
                                match handle.block_on(self.rx.recv()) {
                                    Ok(data) => break data,
                                    Err(Closed) => {
                                        eprintln!("channel closed");
                                        return Ok(count);
                                    }
                                    Err(Lagged(x)) => eprintln!("lagged {x}"),
                                }
                            }
                        }
                    }
                };

                if self.buf.is_empty() {
                    eprintln!("recv empty buf");
                    return Ok(count);
                };
                self.cur = 0;
            }
            *dst = self.buf[self.cur];
            self.cur += 1;
            count += 1;
        }
        Ok(count)
    }
}
impl std::io::Seek for AudioRx {
    fn seek(&mut self, _: std::io::SeekFrom) -> std::io::Result<u64> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "not seekable audio",
        ))
    }
}

impl MediaSource for AudioRx {
    fn is_seekable(&self) -> bool {
        false
    }
    fn byte_len(&self) -> Option<u64> {
        None
    }
}
