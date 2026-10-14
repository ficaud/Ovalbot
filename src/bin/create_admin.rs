use std::env;

use ovalbot::common::db;
use ovalbot::services::auth::hash_password;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut email = None;
    let mut password = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--email" => email = iter.next().cloned(),
            "--password" => password = iter.next().cloned(),
            _ => usage(),
        }
    }

    let (Some(email), Some(password)) = (email, password) else {
        usage();
    };

    let email = email.trim().to_lowercase();
    if !email.contains('@') {
        eprintln!("error: invalid email");
        std::process::exit(1);
    }
    if password.len() < 8 {
        eprintln!("error: password must be at least 8 characters");
        std::process::exit(1);
    }

    let database_url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://ovalbot.db".to_string());
    let pool = db::connect(&database_url).await?;
    db::migrate(&pool).await?;

    let hash = hash_password(&password)?;
    let now = ovalbot::common::time::now();

    let result = sqlx::query(
        "INSERT INTO users (email, password_hash, status, role, created_at, activated_at) \
         VALUES (?, ?, 'active', 'admin', ?, ?)",
    )
    .bind(&email)
    .bind(&hash)
    .bind(&now)
    .bind(&now)
    .execute(&pool)
    .await;

    match result {
        Ok(_) => {
            println!("admin user created: {email}");
            Ok(())
        }
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            eprintln!("error: email already registered");
            std::process::exit(1);
        }
        Err(e) => Err(e.into()),
    }
}

fn usage() -> ! {
    eprintln!("usage: create_admin --email <email> --password <password>");
    std::process::exit(1);
}
