use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{LazyLock, Mutex};

static IMAGE_MAP: LazyLock<Mutex<HashMap<PathBuf, Image>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug)]
pub struct Image {
    tag: String,
}

impl Image {
    pub fn build(dockerfile: impl AsRef<Path>) -> Self {
        let dockerfile = dockerfile.as_ref();
        let mut image_map = IMAGE_MAP.lock().unwrap();

        if let Some(image) = image_map.get(dockerfile) {
            return image.clone();
        }

        let tag = format!(
            "beam-init-test-{}",
            dockerfile.file_stem().unwrap().display()
        );

        Command::new("docker")
            .arg("build")
            .arg("-t")
            .arg(&tag)
            .arg("-f")
            .arg(dockerfile)
            .arg(".")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
            .unwrap();

        let image = Image { tag };
        image_map.insert(dockerfile.to_owned(), image.clone());
        image
    }

    pub fn run(&self, script_path: &str) -> Container {
        let mut cmd = Command::new("docker");

        cmd.arg("run").arg("-i").arg("--rm");
        cmd.arg("-v")
            .arg(format!("{script_path}:/mnt/script.py:ro"));
        cmd.arg(&self.tag).arg("python3").arg("/mnt/script.py");
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        Container {
            child: cmd.spawn().unwrap(),
        }
    }
}

pub struct Container {
    child: Child,
}

impl Container {
    pub fn wait(self) -> Output {
        let output = self.child.wait_with_output().unwrap();
        if !output.status.success() {
            panic!(
                "program failed with {}\nstdout:\n{}\n\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }

        println!(
            "stdout:\n{}\n\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        output
    }

    pub fn wait_expect_code(self, exit_code: i32) -> Output {
        let output = self.child.wait_with_output().unwrap();
        if output.status.code() != Some(exit_code) {
            panic!(
                "program exited with {}, expected exit code {exit_code}\nstdout:\n{}\n\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
        output
    }
}
