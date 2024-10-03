use postgres::{Client, NoTls};
use postgres::Error as PostgresError;
use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};
use std::env::{var, VarError};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct User {
    id: Option<i32>,
    name: String,
    email: String
}

const OK_RESPONSE: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n";
const NOT_FOUND_RESPONSE: &str = "HTTP/1.1 404 NOT FOUND\r\nContent-Type: application/json\r\n\r\n";
const INTERNAL_SERVER_ERROR_RESPONSE: &str = "HTTP/1.1 500 INTERNAL SERVER ERROR\r\nContent-Type: application/json\r\n\r\n";



fn main() {
    if let Err(e) = set_database() {
        panic!("Error setting up database: {:?}", e);
    }

    //start server
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
    println!("Server started at 127.0.0.1:8080");

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                handle_client(s);
            },
            Err(e) => {
                println!("Error: {:?}", e);
            }
        } 
    }
}

fn handle_client(mut stream:TcpStream) {
    let mut buffer = [0; 1024];
    let mut request = String::new();

    let db_url = match var("DATABASE_URL") {
        Ok(val) => val,
        Err(_e) => panic!("DATABASE_URL is not set")
    };

    match stream.read(&mut buffer) {
        Ok(size) => {
            request.push_str(String::from_utf8_lossy(&buffer[..size]).as_ref());

            let (status_line, content): (&str, String) = match &*request {
                r if request.starts_with("GET /users") => {
                    match (Client::connect(&db_url, NoTls)) {
                        (Ok(mut client)) => {
                            match client.query("SELECT * FROM users", &[]) {
                                Ok(rows) => {
                                    let mut users:Vec<User> = Vec::new();
                                    for row in rows {
                                        let user = User {
                                            id: row.get(0),
                                            name: row.get(1),
                                            email: row.get(2)
                                        };
                                        users.push(user);
                                    }
                                    (OK_RESPONSE, serde_json::to_string(&users).unwrap())
                                },
                                Err(e) => (NOT_FOUND_RESPONSE, "USER NOT FOUND".to_string()),
                            }
                        },
                        _ => (INTERNAL_SERVER_ERROR_RESPONSE, "Internal Server Error".to_string()),
                    }
                },
                r if request.starts_with("POST /user") => {
                     match (get_user_request_body(&r), Client::connect(&db_url, NoTls)) {
                        (Ok(user), Ok(mut client)) => {
                            client.execute(
                                "INSERT INTO users (name, email) VALUES ($1, $2)",
                                &[&user.name, &user.email] 
                            ).unwrap();
                            (OK_RESPONSE, "User created".to_string())
                        },
                        (Err(validation), Ok(_)) => {
                            println!("Error: {:?}", validation);
                            (INTERNAL_SERVER_ERROR_RESPONSE, "Internal Server Error".to_string())
                        },
                        _ => (INTERNAL_SERVER_ERROR_RESPONSE, "Internal Server Error".to_string()),
                    }
                },
                r if request.starts_with("GET /user") => {
                    match(get_id(r).parse::<i32>(), Client::connect(&db_url, NoTls)) {
                        (id, Ok(mut client)) => {
                            match &id {
                                Ok(i) => {
                                    match client.query("SELECT * FROM users WHERE id=$1", &[&i]) {
                                        Ok(rows) => {
                                            if rows.len() == 0 {
                                                (NOT_FOUND_RESPONSE, "USER NOT FOUND".to_string())
                                            } else {
                                                let row = rows.get(0).unwrap();
                                                let user = User {
                                                    id: row.get(0),
                                                    name: row.get(1),
                                                    email: row.get(2)
                                                };
                                                (OK_RESPONSE, serde_json::to_string(&user).unwrap())
                                            }
                                        },
                                        Err(e) => (NOT_FOUND_RESPONSE, "USER NOT FOUND".to_string()),   
                                    }
                                },
                                Err(e) => (INTERNAL_SERVER_ERROR_RESPONSE, "Internal Server Error".to_string()),
                            }
                        },
                        _ => (INTERNAL_SERVER_ERROR_RESPONSE, "Internal Server Error".to_string()),
                    }
                },
                r if request.starts_with("PUT /user") => {
                    match(get_id(r).parse::<i32>(), get_user_request_body(&r), Client::connect(&db_url, NoTls)) {
                        (id, Ok(user), Ok(mut client)) => {
                            match &id {
                                Ok(i) => {
                                    match client.execute("UPDATE users SET name=$1, email=$2 WHERE id=$3", &[&user.name, &user.email, &i]) {
                                        Ok(_) => (OK_RESPONSE, "User updated".to_string()),
                                        Err(e) => (NOT_FOUND_RESPONSE, "USER NOT FOUND".to_string()),
                                    }
                                },
                                Err(e) => (INTERNAL_SERVER_ERROR_RESPONSE, "Internal Server Error".to_string()),
                            }
                        },
                        _ => (INTERNAL_SERVER_ERROR_RESPONSE, "Internal Server Error".to_string()),
                    }
                },
                r if request.starts_with("DELETE /user") => {
                    match(get_id(r).parse::<i32>(), Client::connect(&db_url, NoTls)) {
                        (id, Ok(mut client)) => {
                            match &id {
                                Ok(i) => {
                                    match client.execute("DELETE FROM users WHERE id=$1", &[&i]) {
                                        Ok(rows_affected) => {
                                            if rows_affected == 0 {
                                                (NOT_FOUND_RESPONSE, "USER NOT FOUND".to_string())
                                            } else {
                                                (OK_RESPONSE, "User deleted".to_string())
                                            }
                                        },
                                        Err(e) => (NOT_FOUND_RESPONSE, "USER NOT FOUND".to_string()),
                                    }
                                },
                                Err(e) => (INTERNAL_SERVER_ERROR_RESPONSE, "Internal Server Error".to_string()),
                            }
                        },
                        _ => (INTERNAL_SERVER_ERROR_RESPONSE, "Internal Server Error".to_string()),
                    }
                },
                _ => (NOT_FOUND_RESPONSE, "Not Found".to_string())
            };
            stream.write_all(format!("{}{}", status_line, content).as_bytes()).unwrap();
        },
        Err(e) => {
            println!("Error reading from stream: {:?}", e);
        }
    }

}

fn set_database() -> Result<(), PostgresError> {
    let db_url = match var("DATABASE_URL") {
        Ok(val) => val,
        Err(_e) => panic!("DATABASE_URL is not set"),
    };
    println!("DATABASE_URL: {}", &db_url);
    let mut client = Client::connect(&db_url, NoTls)?;
    let result:Result<(), PostgresError> = match client.batch_execute("
        CREATE TABLE IF NOT EXISTS users (
            id SERIAL PRIMARY KEY,
            name VARCHAR NOT NULL,
            email VARCHAR NOT NULL
        )
    ") {
        Ok(_) => Ok(()),
        Err(e) => return Err(e),
    };
    result
}

fn get_id(request: &str) -> &str {
    request.split("/").nth(2).unwrap_or_default().split_whitespace().next().unwrap_or_default()
}

fn get_user_request_body(request: &str) -> Result<User, serde_json::Error> {
    serde_json::from_str(request.split("\r\n\r\n").last().unwrap_or_default())
}
