use std::sync::Arc;

use audio::AudioServiceProvider;
use poise::serenity_prelude as serenity;
pub mod types;
pub mod commands;
pub mod audio;
/// Displays your or another user's account creation date
use commands::*;
use ::serenity::all::GatewayIntents;
use songbird::{driver::DecodeMode, Songbird};
use tokio::sync::mpsc;
use types::Data;


fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .init();
    if let Err(e) = run() {
        tracing::error!("Failed to run bot: {}", e);
        std::process::exit(1);
    }
    Ok(())
}
#[tokio::main]
async fn run() -> anyhow::Result<()> {
    let val = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    let token: Vec<_> = val.split(";").collect();
    let mut intents = serenity::GatewayIntents::all();
    intents.remove(GatewayIntents::GUILD_PRESENCES);
    intents.remove(GatewayIntents::GUILD_MEMBERS);
    intents.remove(GatewayIntents::MESSAGE_CONTENT);
    let songbird_config = songbird::Config::default()
        .decode_mode(DecodeMode::Decode);
    let songbirds = (0..token.len()).map(|_| Songbird::serenity_from_config(songbird_config.clone())).collect();
    let (tx, rx) = mpsc::channel(10);
    let volume_map = Default::default();
    let vm = Arc::clone(&volume_map);
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![ping(), user_info()],
            ..Default::default()
        })
        .setup(move |ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data::new(tx, vm))
            })
        })
        .build();
    let mut clients = Vec::new();
    let sb = Arc::clone(&songbirds);
    let songbird = Arc::clone(&songbirds[0]);
    let client = serenity::ClientBuilder::new(&token[0], intents)
        .framework(framework)
        .voice_manager_arc(songbird)
        .await?;
    let cache = Arc::clone(&client.cache);
    let am = AudioServiceProvider::new(sb, rx, cache, volume_map);
    clients.push(client);
    for i in 1..token.len() {
        let songbird = Arc::clone(&songbirds[i]);
        let client = serenity::ClientBuilder::new(&token[i], intents)
            .voice_manager_arc(songbird)
            .await?;
        clients.push(client);
    }
    let _ = am.run();
    let handles: Vec<_> = clients.into_iter().map(|mut c| tokio::spawn(async move {c.start().await})).collect();
    futures::future::try_join_all(handles).await?
        .into_iter()
        .collect::<Result<Vec<()>, serenity::Error>>()?;
    Ok(())
}
