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
fn freeze_thaw_parent_with_different_pgid() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_freeze_thaw_parent_with_different_pgid.py")
        .wait();
}

#[test]
fn api_non_existent_service() {
    docker_harness::Image::build("test.Dockerfile")
        .run("./tests/api_non_existent_service.py")
        .wait();
}
