#[derive(Debug, Clone)]
pub struct Data{}

pub type Ctx<'a> = poise::Context<'a, Data, anyhow::Error>;

