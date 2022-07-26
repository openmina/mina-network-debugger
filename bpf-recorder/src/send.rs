use core::{mem, ptr};

use typenum::{Bit, Shleft, Unsigned};

use bpf_recorder::Event;

use ebpf_kern::RingBufferRef;

#[inline(always)]
pub fn sized<S, K>(rb: &mut RingBufferRef, mut event: Event, data: *const u8) -> Result<(), i32>
where
    S: Unsigned,
    K: Bit,
{
    use ebpf_kern::helpers;

    if event.size > 0 && event.size <= S::U64 as i32 {
        if let Ok(mut buffer) = rb.reserve(S::U64 as usize + mem::size_of::<Event>()) {
            let p_buffer = buffer.as_mut().as_mut_ptr() as *mut Event;

            let to_copy = (((event.size - 1) as u32) & (S::U32 - 1)) + 1;
            let result = unsafe {
                if K::BOOL {
                    helpers::probe_read_kernel(
                        p_buffer.offset(1) as *mut _,
                        to_copy,
                        data as *const _,
                    )
                } else {
                    helpers::probe_read_user(
                        p_buffer.offset(1) as *mut _,
                        to_copy,
                        data as *const _,
                    )
                }
            };

            if result > 0 {
                event.size = result as i32;
            }
            unsafe {
                ptr::write(p_buffer, event);
            }

            buffer.submit();
            return Ok(());
        } else {
            event.size = -90;
        }
    }

    let mut buffer = rb.reserve(mem::size_of::<Event>())?;
    unsafe {
        ptr::write(buffer.as_mut().as_mut_ptr() as *mut _, event);
    }
    buffer.submit();
    Ok(())
}

#[inline(always)]
pub fn dyn_sized<K>(rb: &mut RingBufferRef, event: Event, data: *const u8) -> Result<(), i32>
where
    K: Bit,
{
    if event.size <= 0 {
        sized::<typenum::U0, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U8>::I32 {
        sized::<Shleft<typenum::U1, typenum::U8>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U9>::I32 {
        sized::<Shleft<typenum::U1, typenum::U9>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U10>::I32 {
        sized::<Shleft<typenum::U1, typenum::U10>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U11>::I32 {
        sized::<Shleft<typenum::U1, typenum::U11>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U12>::I32 {
        sized::<Shleft<typenum::U1, typenum::U12>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U13>::I32 {
        sized::<Shleft<typenum::U1, typenum::U13>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U14>::I32 {
        sized::<Shleft<typenum::U1, typenum::U14>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U15>::I32 {
        sized::<Shleft<typenum::U1, typenum::U15>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U16>::I32 {
        sized::<Shleft<typenum::U1, typenum::U16>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U17>::I32 {
        sized::<Shleft<typenum::U1, typenum::U17>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U18>::I32 {
        sized::<Shleft<typenum::U1, typenum::U18>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U19>::I32 {
        sized::<Shleft<typenum::U1, typenum::U19>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U20>::I32 {
        sized::<Shleft<typenum::U1, typenum::U20>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U21>::I32 {
        sized::<Shleft<typenum::U1, typenum::U21>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U22>::I32 {
        sized::<Shleft<typenum::U1, typenum::U22>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U23>::I32 {
        sized::<Shleft<typenum::U1, typenum::U23>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U24>::I32 {
        sized::<Shleft<typenum::U1, typenum::U24>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U25>::I32 {
        sized::<Shleft<typenum::U1, typenum::U25>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U26>::I32 {
        sized::<Shleft<typenum::U1, typenum::U26>, K>(rb, event, data)
    } else if event.size <= Shleft::<typenum::U1, typenum::U27>::I32 {
        sized::<Shleft<typenum::U1, typenum::U27>, K>(rb, event, data)
    } else {
        Err(0)
    }
}
