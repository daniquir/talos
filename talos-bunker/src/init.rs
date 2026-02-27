use std::env;
use std::fs;

pub async fn init_bunker() {
    let gpg_id = env::var("GPG_ID").unwrap_or_else(|_| "admin@talos.local".to_string());
    let backup_path = "/root/keys/EMERGENCY_BACKUP.txt";

    // Synchronous execution is acceptable here as this runs only once during container startup
    let check = std::process::Command::new("gpg").args(["--list-secret-keys", &gpg_id]).output().unwrap();
    
    if !check.status.success() {
        println!("\n[!] 🛡️ GENESIS PROTOCOL: GENERATING RSA 4096 MASTER KEY...");
        
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
        fs::write(backup_path, &private_key.stdout).expect("Failed to create backup");

        println!("\n###################################################################");
        println!("# [ATTENTION] KEY GENERATED SUCCESSFULLY                          #");
        println!("# To retrieve the backup (ONE TIME ONLY), execute:                #");
        println!("#                                                                 #");
        println!("# docker exec -it talos_bunker reveal-backup                      #");
        println!("#                                                                 #");
        println!("###################################################################\n");
    }
}