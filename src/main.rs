use std::io;

fn main() {
    let x = 5;
    println!("The secret number is: {}", x);
    let x = "six";
    println!("The secret number is: {}", x);

    const NUMBERS: [i32; 10] = [100_000, 200_000, 3, 4, 5, 6, 7, 8, 9, 10];
    for number in NUMBERS {
        println!("The secret number is: {}", number);
    }
}
