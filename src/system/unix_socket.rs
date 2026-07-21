use std::ffi::{c_int, c_uint};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::{io, mem, ptr};

use libc::{
    CMSG_DATA, CMSG_FIRSTHDR, CMSG_LEN, CMSG_SPACE, MSG_NOSIGNAL, SCM_RIGHTS, SOL_SOCKET, cmsghdr,
    iovec, msghdr, sendmsg,
};

use crate::system::cerr;

union ControlMessage {
    // SAFETY: This is always safe.
    buf: [u8; unsafe { CMSG_SPACE(size_of::<c_int>() as c_uint) as usize }],
    _align: cmsghdr,
}

pub fn socket_send_fd(
    socket: impl AsFd,
    data: &[u8],
    fd: BorrowedFd<'_>,
) -> Result<c_int, io::Error> {
    assert!(
        !data.is_empty(),
        "must send at least a single byte for fds to be sent"
    );

    let mut iobuf = iovec {
        iov_base: data.as_ptr().cast_mut().cast(),
        iov_len: data.len(),
    };

    let mut control = ControlMessage { buf: [0; _] };
    // SAFETY: This is the variant we initialized.
    let control_buf = unsafe { &mut control.buf };

    // SAFETY: Safe to zero-initialize
    let mut header: msghdr = unsafe { mem::zeroed() };
    header.msg_name = ptr::null_mut();
    header.msg_namelen = 0;
    header.msg_iov = &mut iobuf;
    header.msg_iovlen = 1;
    header.msg_control = control_buf.as_mut_ptr().cast();
    header.msg_controllen = control_buf.len() as _;
    header.msg_flags = 0;

    // SAFETY: This only accesses the control message buffer.
    unsafe {
        let cmsg = CMSG_FIRSTHDR(&header);
        (*cmsg).cmsg_len = CMSG_LEN(size_of::<c_int>() as c_uint) as _;
        (*cmsg).cmsg_level = SOL_SOCKET;
        (*cmsg).cmsg_type = SCM_RIGHTS;
        *CMSG_DATA(cmsg).cast::<c_int>() = fd.as_raw_fd();
    }

    // SAFETY: msghdr is correctly initialized and socket is a valid fd.
    unsafe { cerr(sendmsg(socket.as_fd().as_raw_fd(), &header, MSG_NOSIGNAL) as c_int) }
}
