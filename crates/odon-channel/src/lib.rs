pub mod adapter;
pub mod cli;
pub mod desktop;
pub mod telegram;
pub mod whatsapp;

pub use adapter::{ChannelAdapter, IncomingChannelMessage};
pub use cli::CliChannel;
pub use desktop::DesktopChannel;
pub use telegram::TelegramRelay;
pub use whatsapp::WhatsAppChannel;
