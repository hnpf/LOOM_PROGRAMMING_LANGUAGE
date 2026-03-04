use std::fs;
use std::path::Path;

pub fn generate_docs(file_path: &str) -> anyhow::Result<()> {
    let source = fs::read_to_string(file_path)?;
    let mut markdown = String::new();
    let file_name = Path::new(file_path).file_name().unwrap().to_str().unwrap();

    markdown.push_str(&format!("# Documentation for `{}`

", file_name));

    let mut legal = Vec::new();
    let mut metadata = Vec::new();
    let mut sections = Vec::new();
    let mut current_section: Option<(String, Vec<String>)> = None;

    for line in source.lines() {
        if let Some(pos) = line.find("//;") {
            legal.push(line[pos+3..].trim().to_string());
        } else if let Some(pos) = line.find("//.") {
            metadata.push(line[pos+3..].trim().to_string());
        } else if let Some(pos) = line.find("//*") {
            if let Some(section) = current_section.take() {
                sections.push(section);
            }
            current_section = Some((line[pos+3..].trim().to_string(), Vec::new()));
        } else if let Some(pos) = line.find("//!") {
            let msg = format!("**Urgent**: {}", line[pos+3..].trim());
            if let Some(ref mut section) = current_section {
                section.1.push(msg);
            } else {
                sections.push(("Important Notes".to_string(), vec![msg]));
            }
        } else if let Some(pos) = line.find("//?") {
            let msg = format!("*Question/Debug*: {}", line[pos+3..].trim());
            if let Some(ref mut section) = current_section {
                section.1.push(msg);
            }
        } else if let Some(pos) = line.find("//") {
            // check if its not one of the special ones already handled
            let rest = &line[pos+2..];
            if !rest.starts_with(['*', '!', '?', '.', ';']) {
                if let Some(ref mut section) = current_section {
                    let c = rest.trim();
                    if !c.is_empty() {
                        section.1.push(c.to_string());
                    }
                }
            }
        }
    }

    if let Some(section) = current_section {
        sections.push(section);
    }

    if !legal.is_empty() {
        markdown.push_str("## Legal & Licensing
");
        for l in legal {
            markdown.push_str(&format!("- {}
", l));
        }
        markdown.push_str("
");
    }

    if !metadata.is_empty() {
        markdown.push_str("## Metadata
");
        for m in metadata {
            markdown.push_str(&format!("- {}
", m));
        }
        markdown.push_str("
");
    }

    for (title, content) in sections {
        markdown.push_str(&format!("## {}
", title));
        for line in content {
            markdown.push_str(&format!("{}
", line));
        }
        markdown.push_str("
");
    }

    let out_path = format!("{}.md", file_path.replace(".lm", ""));
    fs::write(&out_path, markdown)?;
    println!("Documentation generated at {}", out_path);

    Ok(())
}
