// calculates intersection between two sequences of ranges (represented as tuples)
// implemented for kind of "primitive" copy types, expected to be used with integers
pub fn intersect<'a, 'b, T: Ord + 'static + Copy>(
    a: impl IntoIterator<Item = &'a (T, T)>,
    b: impl IntoIterator<Item = &'b (T, T)>,
) -> Vec<(T, T)> {
    let mut a = a.into_iter();
    let mut b = b.into_iter();
    let mut a_state = None;
    let mut b_state = None;

    let mut result = vec![];

    loop {
        if a_state.is_none() {
            a_state = a.next();
        }
        if b_state.is_none() {
            b_state = b.next();
        }
        if a_state.is_none() || b_state.is_none() {
            break;
        }

        match (a_state, b_state) {
            (Some((a1, a2)), Some((b1, b2))) => {
                let s = std::cmp::max(*a1, *b1);
                let e = std::cmp::min(*a2, *b2);
                if s < e {
                    result.push((s, e));
                }

                if a2 < b2 {
                    a_state = None;
                } else {
                    b_state = None;
                }
            }
            _ => unreachable!(),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::intersect;

    #[test]
    fn test_intersect() {
        let r = intersect([(0, 1)].iter(), [(0, 3)].iter());
        assert_eq!(r.as_slice(), &[(0, 1)])
    }

    #[test]
    fn test_intersect1() {
        let r = intersect([(0, 1)].iter(), [(2, 3)].iter());
        assert_eq!(r.as_slice(), &[])
    }

    #[test]
    fn test_intersect2() {
        let r = intersect([(0, 3), (6, 8)].iter(), [(1, 7)].iter());
        assert_eq!(r.as_slice(), &[(1, 3), (6, 7)])
    }

    #[test]
    fn test_intersect4() {
        let r = intersect([(0, 3), (6, 8)].iter(), [].iter());
        assert_eq!(r.as_slice(), &[])
    }

    #[test]
    fn test_intersect5() {
        let r = intersect([(0, 3), (6, 8)].iter(), [(7, 10)].iter());
        assert_eq!(r.as_slice(), &[(7, 8)])
    }
}
