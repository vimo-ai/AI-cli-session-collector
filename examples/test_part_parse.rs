use std::fs;
use std::path::Path;

fn main() {
    let session_id = "ses_440047fbaffehm5340IYtXHKOZ";
    let msg_dir = "/Users/higuaifan/.local/share/opencode/storage/message";
    let part_dir = "/Users/higuaifan/.local/share/opencode/storage/part";

    let session_msg_dir = Path::new(msg_dir).join(session_id);

    if session_msg_dir.exists() {
        for entry in fs::read_dir(&session_msg_dir).unwrap() {
            let entry = entry.unwrap();
            let msg_file = entry.path();
            let file_name = msg_file.file_name().unwrap().to_string_lossy();

            if !file_name.starts_with("msg_") || !file_name.ends_with(".json") {
                continue;
            }

            // Test message file
            let content = fs::read_to_string(&msg_file).unwrap();
            if let Err(e) = serde_json::from_str::<serde_json::Value>(&content) {
                println!("Message file error: {} - {}", msg_file.display(), e);
            }

            // Test associated part files
            let msg_id = file_name.trim_end_matches(".json");
            let part_msg_dir = Path::new(part_dir).join(msg_id);

            if part_msg_dir.exists() {
                for part_entry in fs::read_dir(&part_msg_dir).unwrap() {
                    let part_entry = part_entry.unwrap();
                    let part_file = part_entry.path();

                    if !part_file.to_string_lossy().ends_with(".json") {
                        continue;
                    }

                    let part_content = fs::read_to_string(&part_file).unwrap();
                    if let Err(e) = serde_json::from_str::<serde_json::Value>(&part_content) {
                        println!("Part file error: {} - {}", part_file.display(), e);
                    }
                }
            }
        }
    }

    println!("Scan complete.");
}
