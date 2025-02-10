use crate::client::ChatClientInternal;
use chat_common::messages::chat_message::MessageKind;
use chat_common::messages::{ChatMessage, Empty, JoinChannel};
use common::slc_commands::ChatClientEvent;
use itertools::Itertools;
use log::info;
use wg_2024::network::NodeId;

const SERVER_NOT_FOUND: &str = "[SYSTEM] Error: Server not found";
const HELP_MESSAGE: &str = r"
[SYSTEM] Commands:
[SYSTEM]    /help - Display this message
[SYSTEM]    /servers - Lists discovered servers
[SYSTEM]    /connect <server_id> - Connect to a server
[SYSTEM]    /register <username> - Register with a server. Username cannot contain spaces or '#' and '@'.
[SYSTEM]    /unregister - Unregister from the current server.
[SYSTEM]    /channels - List all channels available on the server.
[SYSTEM]    /join <channel> - Join a channel. You can only be in one channel at a time.
[SYSTEM]    /leave <channel> - Leave the current channel. You will still receive DMs and system communications.
[SYSTEM]    /msg <user> <text> - Send a direct message to a user.
";
const NOT_CONNECTED_TO_SERVER: &str = "[SYSTEM] Error: Not connected to a server. Use /servers to find servers and /connect <server_id> to connect to a server before registering.";
const USERNAME_DISALLOWED_CHARS: &str =
    "[SYSTEM] Error: Username cannot contain spaces, '#' or '@'";
const USER_NOT_FOUND: &str = "[SYSTEM] Error: User not found";
const NO_ALL_CHAN: &str = "[SYSTEM] Error: No 'all' channel found";
const PLEASE_REGISTER: &str =
    "[SYSTEM] Please set your username with /register <username> and try /msg-ing again.";
const LEAVING_CHAN: &str = "[SYSTEM] Leaving channel...";
const NO_CHAN_CONNECTION: &str = "[SYSTEM] Error: You are not connected to a channel.";
const CHANNEL_DISALLOWED_CHARS: &str =
    "[SYSTEM] Error: Channel name cannot contain spaces, '#' or '@'";
const JOINING_CHAN: &str = "[SYSTEM] Joining channel...";
const CREATING_CHAN: &str = "[SYSTEM] Creating channel...";
const UNREGISTERING: &str = "[SYSTEM] Removing registration...";
const NOT_REGISTERED_ERR: &str = "[SYSTEM] Not registered to this server!";

impl ChatClientInternal {
    pub(crate) fn handle_command(
        &mut self,
        command: &str,
        arg: &str,
        freeform: &str,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        info!(target: format!("Client {}", self.own_id).as_str(), "Handling text command: [{} - {} - {}]", command, arg, freeform);
        match command {
            "register" | "unregister" | "channels" | "join" | "leave" | "msg" => {
                if let Some(server_id) = self.currently_connected_server {
                    self.command_handle_with_required_server(server_id, command, arg, freeform)
                } else {
                    (
                        vec![],
                        vec![ChatClientEvent::MessageReceived(
                            NOT_CONNECTED_TO_SERVER.to_string(),
                        )],
                    )
                }
            }
            "help" => (
                vec![],
                vec![ChatClientEvent::MessageReceived(HELP_MESSAGE.to_string())],
            ),
            "servers" => self.cmd_servers(),
            "connect" => self.cmd_connect(arg),
            _ => (
                vec![],
                vec![ChatClientEvent::MessageReceived(format!(
                    "[SYSTEM] Unknown command {command}. Use /help to list available commands."
                ))],
            ),
        }
    }

    fn command_handle_with_required_server(
        &mut self,
        server_id: NodeId,
        command: &str,
        arg: &str,
        freeform: &str,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        match command {
            "unregister" => self.cmd_unregister(server_id),
            "channels" => self.cmd_channels(server_id),
            "join" => self.cmd_join(server_id, arg),
            "leave" => self.cmd_leave(server_id),
            "msg" => self.cmd_msg(server_id, arg, freeform),
            "register" => self.cmd_register(server_id, arg),
            _ => (
                vec![],
                vec![ChatClientEvent::MessageReceived(format!(
                    "[SYSTEM] Unknown command {command}. Use /help to list available commands."
                ))],
            ),
        }
    }

    fn cmd_connect(&mut self, arg: &str) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        self.channels_list.clear();
        self.currently_connected_server = None;
        self.currently_connected_channel = None;
        match self
            .discovered_servers
            .iter()
            .find(|(id, typ)| *typ == "chat" && id.to_string() == arg)
        {
            Some((id, _)) => {
                self.currently_connected_server = Some(*id);
                self.currently_connected_channel = None;
                (
                    vec![(
                        *id,
                        ChatMessage {
                            own_id: u32::from(self.own_id),
                            message_kind: Some(MessageKind::CliRequestChannels(Empty {})),
                        },
                    )],
                    vec![ChatClientEvent::MessageReceived(format!(
                        "[SYSTEM] Connecting to server {id}"
                    ))],
                )
            }
            None => (
                vec![],
                vec![ChatClientEvent::MessageReceived(
                    SERVER_NOT_FOUND.to_string(),
                )],
            ),
        }
    }

    fn cmd_servers(&mut self) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        let servers_list = self
            .discovered_servers
            .iter()
            .filter(|(_, x)| x.as_str() == "chat")
            .map(|(id, _)| id.to_string())
            .join(", ");
        (
            vec![],
            vec![ChatClientEvent::MessageReceived(format!(
                "[SYSTEM] Available servers: {servers_list}"
            ))],
        )
    }

    fn cmd_register(
        &mut self,
        server_id: NodeId,
        arg: &str,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        if arg.contains(' ') || arg.contains('#') || arg.contains('@') {
            (
                vec![],
                vec![ChatClientEvent::MessageReceived(
                    USERNAME_DISALLOWED_CHARS.to_string(),
                )],
            )
        } else {
            match self.server_usernames.get(&server_id) {
                Some(prev) => (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(format!(
                        "[SYSTEM] Error: Already registered with username {prev}"
                    ))],
                ),
                None => (
                    vec![
                        (
                            server_id,
                            ChatMessage {
                                own_id: u32::from(self.own_id),
                                message_kind: Some(MessageKind::CliRegisterRequest(
                                    arg.to_string(),
                                )),
                            },
                        ),
                        (
                            server_id,
                            ChatMessage {
                                own_id: u32::from(self.own_id),
                                message_kind: Some(MessageKind::CliRequestChannels(Empty {})),
                            },
                        ),
                    ],
                    vec![ChatClientEvent::MessageReceived(format!(
                        "[SYSTEM] Registering with username {arg}"
                    ))],
                ),
            }
        }
    }

    fn cmd_msg(
        &mut self,
        server_id: NodeId,
        arg: &str,
        freeform: &str,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        if self.server_usernames.contains_key(&server_id) {
            let all_channel = self.channels_list.iter().find(|x| x.channel_id == 0x1);
            if let Some(all) = all_channel {
                if let Some(dst_id) = all.connected_clients.iter().find(|x| x.username == arg) {
                    (
                        vec![(
                            server_id,
                            ChatMessage {
                                own_id: u32::from(self.own_id),
                                message_kind: Some(MessageKind::SendMsg(
                                    chat_common::messages::SendMessage {
                                        message: freeform.to_string(),
                                        channel_id: dst_id.id << 32 | 0x8,
                                    },
                                )),
                            },
                        )],
                        vec![],
                    )
                } else {
                    (
                        vec![],
                        vec![ChatClientEvent::MessageReceived(USER_NOT_FOUND.to_string())],
                    )
                }
            } else {
                (
                    vec![],
                    vec![ChatClientEvent::MessageReceived(NO_ALL_CHAN.to_string())],
                )
            }
        } else {
            (
                vec![],
                vec![ChatClientEvent::MessageReceived(
                    PLEASE_REGISTER.to_string(),
                )],
            )
        }
    }

    fn cmd_leave(
        &mut self,
        server_id: NodeId,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        match self.currently_connected_channel {
            Some(..) => {
                self.currently_connected_channel = None;
                (
                    vec![(
                        server_id,
                        ChatMessage {
                            own_id: self.own_id.into(),
                            message_kind: Some(MessageKind::CliLeave(Empty {})),
                        },
                    )],
                    vec![ChatClientEvent::MessageReceived(LEAVING_CHAN.to_string())],
                )
            }
            None => (
                vec![],
                vec![ChatClientEvent::MessageReceived(
                    NO_CHAN_CONNECTION.to_string(),
                )],
            ),
        }
    }

    fn cmd_join(
        &mut self,
        server_id: NodeId,
        arg: &str,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        if arg.contains('#') || arg.contains('@') || arg.contains(' ') {
            (
                vec![],
                vec![ChatClientEvent::MessageReceived(
                    CHANNEL_DISALLOWED_CHARS.to_string(),
                )],
            )
        } else {
            match self.channels_list.iter().find(|x| arg == x.channel_name) {
                Some(channel) => (
                    vec![(
                        server_id,
                        ChatMessage {
                            own_id: u32::from(self.own_id),
                            message_kind: Some(MessageKind::CliJoin(JoinChannel {
                                channel_id: Some(channel.channel_id),
                                channel_name: String::new(),
                            })),
                        },
                    )],
                    vec![ChatClientEvent::MessageReceived(JOINING_CHAN.to_string())],
                ),
                None => (
                    vec![(
                        server_id,
                        ChatMessage {
                            own_id: u32::from(self.own_id),
                            message_kind: Some(MessageKind::CliJoin(JoinChannel {
                                channel_id: None,
                                channel_name: arg.to_string(),
                            })),
                        },
                    )],
                    vec![ChatClientEvent::MessageReceived(CREATING_CHAN.to_string())],
                ),
            }
        }
    }

    fn cmd_channels(
        &mut self,
        server_id: NodeId,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        let chan_list = self
            .channels_list
            .iter()
            .filter(|x| x.channel_is_group && x.channel_id != 0x1)
            .map(|x| format!("#{}", x.channel_name))
            .join(",");
        let user_list = self
            .channels_list
            .iter()
            .find(|x| x.channel_id == 0x1)
            .map_or(String::new(), |x| {
                x.connected_clients
                    .iter()
                    .map(|x| format!("@{}", x.username))
                    .join(",")
            });
        let msg = format!(
            "[SYSTEM] Available channels: {chan_list}\n[SYSTEM] Available IMs: {user_list}"
        );
        (
            vec![(
                server_id,
                ChatMessage {
                    own_id: u32::from(self.own_id),
                    message_kind: Some(MessageKind::CliRequestChannels(Empty {})),
                },
            )],
            vec![ChatClientEvent::MessageReceived(msg)],
        )
    }

    fn cmd_unregister(
        &mut self,
        server_id: NodeId,
    ) -> (Vec<(NodeId, ChatMessage)>, Vec<ChatClientEvent>) {
        match self.server_usernames.get(&server_id) {
            Some(_) => {
                self.server_usernames.remove(&server_id);
                (
                    vec![(
                        server_id,
                        ChatMessage {
                            own_id: u32::from(self.own_id),
                            message_kind: Some(MessageKind::CliCancelReg(Empty {})),
                        },
                    )],
                    vec![ChatClientEvent::MessageReceived(UNREGISTERING.to_string())],
                )
            }
            None => (
                vec![],
                vec![ChatClientEvent::MessageReceived(
                    NOT_REGISTERED_ERR.to_string(),
                )],
            ),
        }
    }
}
