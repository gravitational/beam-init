// You can use the following inside the container to attach a debugger to beam-init:
// subprocess.check_call(["gdbserver", "--attach", ":1234", "1"])

use std::path::PathBuf;

use crate::docker;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn dockerfile() -> PathBuf {
    workspace_root().join("test.Dockerfile")
}

fn test_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

#[test]
fn self_test_pass() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("self_test_pass.py").to_str().unwrap())
        .wait();
}

#[test]
fn self_test_fail() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("self_test_fail.py").to_str().unwrap())
        .wait_expect_code(1);
}

#[test]
fn init_reaps_zombies() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("init_reaps_zombies.py").to_str().unwrap())
        .wait();
}

#[test]
fn api_start_service() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("api_start_service.py").to_str().unwrap())
        .wait();
}

#[test]
fn api_stop_service() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("api_stop_service.py").to_str().unwrap())
        .wait();
}

#[test]
fn api_stop_service_wrong_user() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(
            test_path("api_stop_service_wrong_user.py")
                .to_str()
                .unwrap(),
        )
        .wait();
}

#[test]
fn api_show_service() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("api_show_service.py").to_str().unwrap())
        .wait();
}

#[test]
fn api_list_services() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("api_list_services.py").to_str().unwrap())
        .wait();
}

#[test]
fn api_service_logs() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("api_service_logs.py").to_str().unwrap())
        .wait();
}

#[test]
fn api_freeze_thaw_service() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("api_freeze_thaw_service.py").to_str().unwrap())
        .wait();
}

#[test]
fn api_non_existent_service() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("api_non_existent_service.py").to_str().unwrap())
        .wait();
}

#[test]
fn api_start_invalid_service() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("api_start_invalid_service.py").to_str().unwrap())
        .wait();
}

#[test]
fn api_restart_service() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("api_restart_service.py").to_str().unwrap())
        .wait();
}

#[test]
fn process_group_kill() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("process_group_kill.py").to_str().unwrap())
        .wait();
}

#[test]
fn api_readiness_http_server() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("api_liveness_http_server.py").to_str().unwrap())
        .wait();
}

#[test]
fn api_liveness_max_retries() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("api_liveness_max_retries.py").to_str().unwrap())
        .wait();
}

#[test]
fn prefix_match() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("prefix_match.py").to_str().unwrap())
        .wait();
}

#[test]
fn completion() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("completion.py").to_str().unwrap())
        .wait();
}

#[test]
fn pty_attach_race() {
    docker::Image::build(dockerfile(), workspace_root())
        .run(test_path("pty_attach_race.py").to_str().unwrap())
        .wait();
}
