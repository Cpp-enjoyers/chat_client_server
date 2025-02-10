use crate::client::ChatClientInternal;
use chat_common::messages::chat_message::MessageKind;
use chat_common::messages::{ChatMessage, Empty, JoinChannel};
use common::slc_commands::ChatClientEvent;
use itertools::Itertools;
use log::info;
use wg_2024::network::NodeId;

impl ChatClientInternal {
    pub(crate) fn handle_message(
        &mut self,
        message: &str,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        info!(target: format!("Client {}", self.own_id).as_str(), "Handling text message: {:?}", message);
        if message.starts_with('/') {
            let msg = message.chars().skip(1).collect::<String>();
            let (cmd, remainder) = msg.split_once(' ').unwrap_or((msg.as_str(), ""));
            info!(target: format!("Client {}", self.own_id).as_str(), "First split: {cmd}, {remainder}");
            let (arg, freeform) = remainder.split_once(' ').unwrap_or((remainder, ""));
            info!(target: format!("Client {}", self.own_id).as_str(), "First split: {arg}, {remainder}");
            return self.handle_command(cmd, arg, freeform);
        }
        self.handle_text_message(message)
    }

    fn handle_text_message(
        &mut self,
        message: &str,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        match (self.currently_connected_server, self.currently_connected_channel) {
            (Some(connected_server), Some(connected_channel)) => {
                if self.server_usernames.contains_key(&connected_server) {
                    (
                        vec![(
                            connected_server,
                            ChatMessage {
                                own_id: u32::from(self.own_id),
                                message_kind: Some(MessageKind::SendMsg(
                                    chat_common::messages::SendMessage {
                                        message: message.to_string(),
                                        channel_id: connected_channel,
                                    },
                                )),
                            },
                        )],
                        vec![],
                    )
                } else {
                    (
                        vec![],
                        vec![ChatClientEvent::MessageReceived(
                            "[SYSTEM] Please set your username with /register <username> and try /join-ing again.".to_string(),
                        )],
                    )
                }
            }
            (Some(_), None) => {
                (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(
                        "[SYSTEM] You are not in a channel. Use /channels to see available channels and /join <channel_id> to join one.".to_string(),
                    )],
                )
            }
            (None, _) => {
                (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(
                        "[SYSTEM] You are not connected to a server. Use /servers to find servers and /connect <server_id> to connect to a server.".to_string(),
                    )],
                )
            }
        }
    }
}
