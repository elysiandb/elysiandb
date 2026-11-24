use std::io;

#[cfg(target_os = "linux")]
pub fn os_random(buf: &mut [u8]) -> io::Result<()> {
    let mut done = 0;
    while done < buf.len() {
        let rest = &mut buf[done..];
        let n = unsafe { libc::getrandom(rest.as_mut_ptr().cast(), rest.len(), 0) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        done += n as usize;
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn os_random(buf: &mut [u8]) -> io::Result<()> {
    for chunk in buf.chunks_mut(256) {
        if unsafe { libc::getentropy(chunk.as_mut_ptr().cast(), chunk.len()) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(())
}
