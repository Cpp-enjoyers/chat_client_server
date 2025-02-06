use chat_common::messages::ChatMessage;
use chat_common::packet_handling::{CommandHandler, PacketHandler};
use common::slc_commands::{ChatClientCommand, ChatClientEvent, ClientCommand};
use wg_2024::packet;
use wg_2024::packet::NodeType;

struct ChatClientInternal {

}

impl CommandHandler<ChatClientCommand, ChatClientEvent> for ChatClientInternal {
    fn get_node_type() -> crate::server::packet::NodeType {
        NodeType::Server
    }

    fn handle_protocol_message(&mut self, message: ChatMessage) -> chat_common::packet_handling::HandlerFunction<ChatClientCommand, ChatClientEvent, Self>
    where
        Self: Sized
    {
        todo!()
    }

    fn report_sent_packet(&mut self, packet: packet::Packet) -> chat_common::packet_handling::HandlerFunction<ChatClientCommand, ChatClientEvent, Self>
    where
        Self: Sized
    {
        todo!()
    }

    fn handle_controller_command(&mut self, command: ChatClientCommand) -> chat_common::packet_handling::HandlerFunction<ChatClientCommand, ChatClientCommand, Self>
    where
        Self: Sized
    {
        todo!()
    }

    fn new() -> Self
    where
        Self: Sized
    {
        todo!()
    }
}

pub type ChatClient = PacketHandler<ChatClientCommand, ChatClientEvent, ChatClientInternal>;