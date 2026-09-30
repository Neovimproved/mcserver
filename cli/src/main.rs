use mcserver_core::Message;

fn main() {
    let my_message = Message::new("hello".to_string());

    Message::new();
    println!("Yes {my_message}");
}
