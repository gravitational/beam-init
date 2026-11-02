use crate::docker::Image;

#[test]
fn dont_panic_the_reaper() {
    Image::build("test.Dockerfile")
        .run("regression_tel_q426_01.py")
        .wait();
}
