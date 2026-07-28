#[test]
fn self_test_pass() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/self_test_pass.py")
        .wait();
}

#[test]
fn self_test_fail() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/self_test_fail.py")
        .wait_expect_code(1);
}

#[test]
fn init_reaps_zombies() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/init_reaps_zombies.py")
        .wait();
}

#[test]
fn api_start_service() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_start_service.py")
        .wait();
}

#[test]
fn api_stop_service() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_stop_service.py")
        .wait();
}

#[test]
fn api_stop_service_wrong_user() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_stop_service_wrong_user.py")
        .wait();
}

#[test]
fn api_show_service() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_show_service.py")
        .wait();
}

#[test]
fn api_list_services() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_list_services.py")
        .wait();
}

#[test]
fn api_service_logs() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_service_logs.py")
        .wait();
}

#[test]
fn api_freeze_thaw_service() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_freeze_thaw_service.py")
        .wait();
}

#[test]
fn api_non_existent_service() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_non_existent_service.py")
        .wait();
}

#[test]
fn api_start_invalid_service() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_start_invalid_service.py")
        .wait();
}

#[test]
fn api_restart_service() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_restart_service.py")
        .wait();
}

#[test]
fn process_group_kill() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/process_group_kill.py")
        .wait();
}

#[test]
fn api_readiness_http_server() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_liveness_http_server.py")
        .wait();
}

#[test]
fn api_liveness_max_retries() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_liveness_max_retries.py")
        .wait();
}

#[test]
fn environment_file() {
    docker_harness::Image::build("test.Dockerfile")
        .run_with_init_args(
            "./tests/environment_file.py",
            &["--environment-file", "/etc/beam-env"],
        )
        .wait();
}
