use sea_orm::{Database, DatabaseConnection};
use std::env;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    #[allow(dead_code)]
    pub jwt_secret: String,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        // SQLite를 기본값으로 사용 (테스트용)
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:./test.db?mode=rwc".to_string());
        println!("database_url: {}", database_url);

        Self {
            database_url,
            jwt_secret: env::var("JWT_SECRET")
                .unwrap_or_else(|_| "your-secret-key-change-in-production".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "8000".to_string())
                .parse()
                .expect("PORT must be a valid number"),
        }
    }
}

pub async fn create_db_connection(database_url: &str) -> DatabaseConnection {
    Database::connect(database_url)
        .await
        .expect("Failed to connect to database")
}
