use std::io;

fn main() {
    let mut x = 5;
    println!("The secret number is: {}", x);
    let x = 6;
    println!("The secret number is: {}", x);

    const NUMBERS: [i32; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    for number in NUMBERS {
        println!("The secret number is: {}", number);
    }
}
