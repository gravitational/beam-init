use crate::docker::Image;

#[test]
fn tel_q426_01() {
    Image::build("test.Dockerfile")
        .run("regression_tel_q426_01.py")
        .wait();
}

#[test]
fn tel_q426_05() {
    Image::build("test.Dockerfile")
        .run("regression_tel_q426_05.py")
        .wait();
}
