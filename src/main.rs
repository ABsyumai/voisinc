use poise::serenity_prelude as serenity;
pub mod types;
pub mod commands;
pub mod audio;
/// Displays your or another user's account creation date
use commands::*;
use ::serenity::all::GatewayIntents;
use types::Data;
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .init();
    let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    let mut intents = serenity::GatewayIntents::all();
    intents.remove(GatewayIntents::GUILD_PRESENCES);
    intents.remove(GatewayIntents::GUILD_MEMBERS);
    intents.remove(GatewayIntents::MESSAGE_CONTENT);

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![ping(), user_info()],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {})
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;
    client.unwrap().start().await.unwrap();
}
