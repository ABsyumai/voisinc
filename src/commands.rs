use crate::types::Ctx;
use poise::serenity_prelude::*;
use tracing::info;

type Result = anyhow::Result<()>;


#[poise::command(slash_command)]
#[tracing::instrument(name="ping", skip(ctx), fields(author=ctx.author().id.get()))]
pub async fn ping(
    ctx: Ctx<'_>,
) -> Result {
    info!("pong");
    ctx.send(
        poise::CreateReply::default()
            .content("Pong")
            .ephemeral(true)
            .reply(true)
    ).await?;
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
    info!("{}",&response);
    ctx.send(
        poise::CreateReply::default()
            .content(response)
            .ephemeral(true)
    ).await?;
    Ok(())
}
