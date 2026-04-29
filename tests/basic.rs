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
