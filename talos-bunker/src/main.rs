use axum::{routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::process::Stdio;
// CAMBIO CLAVE: Usamos la versión de Tokio para procesos y entrada/salida
use tokio::process::Command;
use tokio::io::AsyncWriteExt;

#[derive(Deserialize)]
struct CryptTask {
    payload: String,
    passphrase: String,
    mode: String,
}

#[derive(Serialize)]
struct CryptResponse {
    result: String,
}

async fn process_gpg(Json(req): Json<CryptTask>) -> Json<CryptResponse> {
    let gpg_id = env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());
    let mut args = vec!["--batch", "--pinentry-mode", "loopback", "--passphrase-fd", "0"];
    
    if req.mode == "decrypt" {
        args.push("-d");
    } else {
        args.extend(["-e", "-r", &gpg_id, "--armor"]); 
    }

    // Command ahora es tokio::process::Command
    let mut child = Command::new("gpg")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Error al iniciar GPG");

    let mut stdin = child.stdin.take().unwrap();
    let input = format!("{}\n{}", req.passphrase, req.payload);
    
    // Ahora write_all y .await funcionarán correctamente
    stdin.write_all(input.as_bytes()).await.unwrap();
    drop(stdin);

    // wait_with_output ahora es un Future de Tokio
    let output = child.wait_with_output().await.expect("Error en GPG");
    
    Json(CryptResponse {
        result: String::from_utf8_lossy(&output.stdout).to_string(),
    })
}

async fn init_bunker() {
    let gpg_id = env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());
    let backup_path = "/root/keys/EMERGENCY_BACKUP.txt";

    // En el inicio podemos usar std::process porque no necesitamos asincronía pura aún
    let check = std::process::Command::new("gpg").args(["--list-secret-keys", &gpg_id]).output().unwrap();
    
    if !check.status.success() {
        println!("\n[!] 🛡️ PROTOCOLO GÉNESIS: GENERANDO LLAVE MAESTRA RSA 4096...");
        
        let gen_params = format!(
            "%no-protection\nKey-Type: RSA\nKey-Length: 4096\nName-Email: {}\nExpire-Date: 0\n%commit\n",
            gpg_id
        );
        fs::write("/tmp/gpg_gen", gen_params).unwrap();

        std::process::Command::new("gpg").args(["--batch", "--generate-key", "/tmp/gpg_gen"]).status().unwrap();
        
        let private_key = std::process::Command::new("gpg")
            .args(["--export-secret-keys", "--armor", &gpg_id])
            .output().unwrap();
        
        fs::create_dir_all("/root/keys").unwrap();
        fs::write(backup_path, &private_key.stdout).expect("Error al crear backup");

        println!("\n###################################################################");
        println!("# [ATENCIÓN] LLAVE GENERADA CON ÉXITO                             #");
        println!("# Para obtener el backup (SOLO UNA VEZ), ejecuta:                 #");
        println!("#                                                                 #");
        println!("# docker exec -it talos_bunker reveal-backup                      #");
        println!("#                                                                 #");
        println!("###################################################################\n");
    }
}

#[tokio::main]
async fn main() {
    init_bunker().await;
    let app = Router::new().route("/process", post(process_gpg));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:5000").await.unwrap();
    println!("🛡️ Búnker GPG activo y blindado en puerto 5000");
    axum::serve(listener, app).await.unwrap();
}