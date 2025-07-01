use coins_crypto::{aggregate, sign, verify, verify_aggregate, SecretKey};

#[test]
fn sign_and_verify() {
    let sk = SecretKey::random();
    let pk = sk.public_key();
    let msg = b"hello world";

    let sig = sign(&sk, msg);
    assert!(verify(&pk, msg, &sig));

    // Should fail for different message
    assert!(!verify(&pk, b"hello mars", &sig));
}

#[test]
fn aggregate_signatures() {
    const N: usize = 8;
    let mut sigs = Vec::with_capacity(N);
    let mut pairs = Vec::with_capacity(N);

    for i in 0..N {
        let sk = SecretKey::random();
        let msg = format!("msg{}", i);
        let sig = sign(&sk, msg.as_bytes());
        sigs.push(sig);
        pairs.push((sk.public_key(), msg));
    }

    let sigma = aggregate(sigs.iter());
    // convert to iterator over (&G1, &[u8])
    let check_iter = pairs
        .iter()
        .map(|(pk, m)| (pk, m.as_bytes()));

    assert!(verify_aggregate(check_iter, &sigma));
} 