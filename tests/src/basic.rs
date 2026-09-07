use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::docker::{Image, RunOptions};

#[test]
fn self_test_pass() {
    Image::build("test.Dockerfile")
        .run("self_test_pass.py")
        .wait();
}

#[test]
fn self_test_fail() {
    Image::build("test.Dockerfile")
        .run("self_test_fail.py")
        .wait_expect_code(1);
}

#[test]
fn init_reaps_zombies() {
    Image::build("test.Dockerfile")
        .run("init_reaps_zombies.py")
        .wait();
}

#[test]
fn api_start_service() {
    Image::build("test.Dockerfile")
        .run("api_start_service.py")
        .wait();
}

#[test]
fn api_service_environment() {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/environment");
    let mut env_files: Vec<OsString> = std::fs::read_dir(&fixture_dir)
        .unwrap()
        .map(|f| f.unwrap())
        .filter(|f| f.file_type().unwrap().is_file())
        .map(|f| f.file_name())
        .collect();
    env_files.sort();

    let container_dir = Path::new("/mnt/env/");
    let args = env_files.into_iter().flat_map(|f| {
        let path = container_dir.join(f).into_os_string();
        [OsString::from("--environment-file"), path]
    });

    let options = RunOptions::default()
        .mount(fixture_dir, container_dir.into())
        .beam_init_args(args);

    Image::build("test.Dockerfile")
        .run_with_options("api_service_environment.py", options)
        .wait();
}

#[test]
fn api_stop_service() {
    Image::build("test.Dockerfile")
        .run("api_stop_service.py")
        .wait();
}

#[test]
fn api_stop_service_wrong_user() {
    Image::build("test.Dockerfile")
        .run("api_stop_service_wrong_user.py")
        .wait();
}

#[test]
fn api_show_service() {
    Image::build("test.Dockerfile")
        .run("api_show_service.py")
        .wait();
}

#[test]
fn api_list_services() {
    Image::build("test.Dockerfile")
        .run("api_list_services.py")
        .wait();
}

#[test]
fn api_service_logs() {
    Image::build("test.Dockerfile")
        .run("api_service_logs.py")
        .wait();
}

#[test]
fn api_freeze_thaw_service() {
    Image::build("test.Dockerfile")
        .run("api_freeze_thaw_service.py")
        .wait();
}

#[test]
fn api_non_existent_service() {
    Image::build("test.Dockerfile")
        .run("api_non_existent_service.py")
        .wait();
}

#[test]
fn api_start_invalid_service() {
    Image::build("test.Dockerfile")
        .run("api_start_invalid_service.py")
        .wait();
}

#[test]
fn api_restart_service() {
    Image::build("test.Dockerfile")
        .run("api_restart_service.py")
        .wait();
}

#[test]
fn process_group_kill() {
    Image::build("test.Dockerfile")
        .run("process_group_kill.py")
        .wait();
}

#[test]
fn api_readiness_http_server() {
    Image::build("test.Dockerfile")
        .run("api_liveness_http_server.py")
        .wait();
}

#[test]
fn api_liveness_max_retries() {
    Image::build("test.Dockerfile")
        .run("api_liveness_max_retries.py")
        .wait();
}

#[test]
fn prefix_match() {
    Image::build("test.Dockerfile")
        .run("prefix_match.py")
        .wait();
}

#[test]
fn completion() {
    Image::build("test.Dockerfile").run("completion.py").wait();
}

#[test]
fn pty_attach_race() {
    Image::build("test.Dockerfile")
        .run("pty_attach_race.py")
        .wait();
}

#[test]
fn pty_attach_wrong_user() {
    Image::build("test.Dockerfile")
        .run("pty_attach_wrong_user.py")
        .wait();
}

#[test]
fn pty_owner() {
    Image::build("test.Dockerfile").run("pty_owner.py").wait();
}
