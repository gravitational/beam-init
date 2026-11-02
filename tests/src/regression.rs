use crate::docker::Image;

#[test]
fn tel_q426_01() {
    Image::build("test.Dockerfile")
        .run("regression_tel_q426_01.py")
        .wait();
}

#[test]
fn tel_q426_02() {
    Image::build("test.Dockerfile")
        .run("regression_tel_q426_02.py")
        .wait();
}

#[test]
fn tel_q426_06() {
    Image::build("test.Dockerfile")
        .run("regression_tel_q426_06.py")
        .wait();
}
