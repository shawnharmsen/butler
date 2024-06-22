use std::io;
use std::cmp::Ordering;
use rand::Rng;

fn main() {
    println!("Enter number");

    let secret = rand::thread_rng().gen_range(1..100);

    println!("The secret number is: {}", secret);

    let mut guess = String::new();
    io::stdin()
        .read_line(&mut guess)
        .expect("Failed to read line");

    let guess: u32 = guess.trim().parse().expect("Not a number");

    println!("You entered: {}", guess);

    match guess.cmp(&secret) {
        Ordering::Less => println!("Too small!"),
        Ordering::Greater => println!("Too big!"),
        Ordering::Equal => println!("You win!"),
    }
}
