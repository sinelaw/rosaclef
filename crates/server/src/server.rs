//! HTTP / WebSocket server (work in progress).
use crate::folder::Folder;
use rosaclef_engine::Engine;
use std::path::PathBuf;

pub struct Config {
    pub folder: Folder,
    pub host: String,
    pub port: u16,
    pub web: PathBuf,
}

pub fn install_plugin_host(_engine: &mut Engine) {}

pub async fn run(_cfg: Config) -> anyhow::Result<()> {
    anyhow::bail!("server not implemented yet")
}
