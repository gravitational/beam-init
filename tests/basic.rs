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
