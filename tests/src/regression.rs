use crate::docker::{Image, RunOptions};

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
fn tel_q426_03() {
    Image::build("test.Dockerfile")
        .run_with_options(
            "regression_tel_q426_03.py",
            RunOptions::default().memory("20m"),
        )
        .wait();
}

#[test]
fn tel_q426_04() {
    Image::build("test.Dockerfile")
        .run("regression_tel_q426_04.py")
        .wait();
}

#[test]
fn tel_q426_06() {
    Image::build("test.Dockerfile")
        .run("regression_tel_q426_06.py")
        .wait();
}
