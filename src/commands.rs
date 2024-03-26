use crate::{
    audio::{AudioCommandError, AudioCommandPayload},
    types::Ctx,
};
use poise::serenity_prelude::*;
use tracing::info;

type Result = anyhow::Result<()>;

#[poise::command(slash_command)]
#[tracing::instrument(name = "join", skip(ctx))]
pub async fn join(ctx: Ctx<'_>) -> Result {
    // let id = ctx.channel_id();
    // let channel = ctx.guild().unwrap().channels.get(&id).unwrap().kind.clone();
    let res = (async {
        let vc;
        let guild_id;
        {
            let guild = ctx.guild().ok_or("not in guild")?;
            guild_id = guild.id;
            vc = guild
                .voice_states
                .get(&ctx.author().id)
                .and_then(|vs| vs.channel_id)
                .ok_or("not in voice channel")?;
        };
        match ctx
            .framework()
            .user_data
            .command(AudioCommandPayload::Join(guild_id, vc))
            .await
        {
            Err(AudioCommandError::ProviderDropped) => {
                tracing::error!("AudioService is doropped");
                return Err("internal error");
            }
            Err(AudioCommandError::BotUsedFull) => return Err("bot used full"),
            Err(_) => return Err("unknown error"),
            Ok(_) => Ok(vc),
        }
    })
    .await;

    let res = match res {
        Err(e) => ctx.say(e).await,
        Ok(id) => ctx.say(format!("Joined to <#{}>", id)).await,
    };
    if let Err(e) = res {
        tracing::warn!("Failed to send message: {}", e);
    }
    Ok(())
}

#[poise::command(slash_command)]
#[tracing::instrument(name="ping", skip(ctx), fields(author=ctx.author().id.get()))]
pub async fn ping(ctx: Ctx<'_>) -> Result {
    info!("pong");
    ctx.send(
        poise::CreateReply::default()
            .content("Pong")
            .ephemeral(true)
            .reply(true),
    )
    .await?;
    Ok(())
}

#[poise::command(context_menu_command = "User information", slash_command)]
#[tracing::instrument(name="user_info", skip(ctx, user), fields(author=ctx.author().id.get(), user = user.id.get()))]
pub async fn user_info(
    ctx: Ctx<'_>,
    #[description = "Discord profile to query information about"] user: User,
) -> Result {
    let response = format!(
        "**Name**: {}\n**Created**: {}",
        user.name,
        user.created_at()
    );
    info!("{}", &response);
    ctx.send(
        poise::CreateReply::default()
            .content(response)
            .ephemeral(true),
    )
    .await?;
    Ok(())
}
