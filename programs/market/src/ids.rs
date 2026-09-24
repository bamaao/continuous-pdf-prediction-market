/// Domain-separated identity digest. Used as the market PDA seed.
fn digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut st = [
        0x736f6d6570736575u64,
        0x646f72616e646f6du64,
        0x6c7967656e657261u64,
        0x7465646279746573u64,
    ];
    for (n, part) in parts.iter().enumerate() {
        st[0] ^= (part.len() as u64).wrapping_add((n as u64) << 32);
        for chunk in part.chunks(8) {
            let mut x = 0u64;
            for (i, b) in chunk.iter().enumerate() {
                x |= (*b as u64) << (8 * i);
            }
            st[0] = st[0].wrapping_add(x).rotate_left(13);
            st[1] ^= st[0];
            st[2] = st[2].wrapping_add(st[1]).rotate_left(17);
            st[3] ^= st[2];
            st[0] = st[0].wrapping_mul(0x9E3779B97F4A7C15);
        }
    }
    let mut out = [0u8; 32];
    for (i, word) in st.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&word.to_le_bytes());
    }
    out
}

pub fn skellam(topic: &[u8; 32], score_scope: u8) -> [u8; 32] {
    digest(&[b"sk", topic.as_ref(), &[score_scope]])
}

pub fn interval(family: u8, topic: &[u8; 32], tag: &[u8; 32]) -> [u8; 32] {
    digest(&[b"iv", &[family], topic.as_ref(), tag.as_ref()])
}

pub fn dirichlet(topic: &[u8; 32], layout: u8, top_n: u8, bins: u16) -> [u8; 32] {
    digest(&[
        b"di",
        topic.as_ref(),
        &[layout],
        &[top_n],
        &bins.to_le_bytes(),
    ])
}

pub fn bernoulli(topic: &[u8; 32], tag: &[u8; 32]) -> [u8; 32] {
    digest(&[b"be", topic.as_ref(), tag.as_ref()])
}

pub fn set_hash(mask: &[u8]) -> [u8; 32] {
    digest(&[b"set", mask])
}

pub fn skellam_ticket(kind: u8, a: i16, b: i16) -> [u8; 32] {
    digest(&[
        b"skset",
        &[kind],
        &a.to_le_bytes(),
        &b.to_le_bytes(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skellam_unique_by_scope() {
        let m = [7u8; 32];
        assert_ne!(skellam(&m, 0), skellam(&m, 1));
    }

    #[test]
    fn interval_splits_family_and_tag() {
        let t = [1u8; 32];
        let a = [2u8; 32];
        let b = [3u8; 32];
        assert_ne!(interval(1, &t, &a), interval(1, &t, &b));
        assert_ne!(interval(1, &t, &a), interval(2, &t, &a));
    }

    #[test]
    fn dirichlet_layout_is_not_a_product_type() {
        let t = [4u8; 32];
        assert_ne!(dirichlet(&t, 0, 0, 0), dirichlet(&t, 2, 0, 8));
    }
}
