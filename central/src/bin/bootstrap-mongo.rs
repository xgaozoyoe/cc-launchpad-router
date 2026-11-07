use mongodb::{bson::doc, Client};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::var("MONGO_PORT")?.parse()?;
    let root = std::env::var("MONGO_ROOT_PASSWORD")?;
    let app = std::env::var("MONGO_APP_PASSWORD")?;
    if ![&root, &app]
        .iter()
        .all(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("Bootstrap requires generated 32-byte hex passwords".into());
    }
    let direct =
        format!("mongodb://127.0.0.1:{port}/?directConnection=true&serverSelectionTimeoutMS=10000");
    let client = Client::with_uri_str(&direct).await?;
    let admin = client.database("admin");
    if !admin
        .run_command(doc! {"hello":1})
        .await?
        .contains_key("setName")
    {
        admin.run_command(doc!{"replSetInitiate":{"_id":"rs0","members":[{"_id":0,"host":format!("127.0.0.1:{port}")}]}}).await?;
    }
    let mut primary = false;
    for _ in 0..120 {
        if admin
            .run_command(doc! {"hello":1})
            .await?
            .get_bool("isWritablePrimary")
            .unwrap_or(false)
        {
            primary = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    if !primary {
        return Err("MongoDB primary election timed out".into());
    }
    let privileged = Client::with_uri_str(format!("mongodb://root:{root}@127.0.0.1:{port}/admin?directConnection=true&serverSelectionTimeoutMS=10000")).await?;
    // A new instance permits first-admin creation through MongoDB's localhost exception.
    if privileged
        .database("admin")
        .run_command(doc! {"connectionStatus":1})
        .await
        .is_err()
    {
        admin
            .run_command(
                doc! {"createUser":"root","pwd":&root,"roles":[{"role":"root","db":"admin"}]},
            )
            .await?;
    }
    let db = privileged.database("launchpad");
    let users = db.run_command(doc! {"usersInfo":"launchpad"}).await?;
    if users.get_array("users")?.is_empty() {
        db.run_command(doc!{"createUser":"launchpad","pwd":&app,"roles":[{"role":"readWrite","db":"launchpad"}]}).await?;
    }
    println!("Isolated MongoDB replica set and application user ready");
    Ok(())
}
