/// Returns a greeting message.
pub fn greet() -> &'static str {
    "Hello from the first crate!"
}


#[docify::export]
pub fn example_to_embed() {
    assert_eq!(2 + 2, 4);
    assert_eq!(2 + 3, 5);
    println!("Example running from first_crate!");
}
