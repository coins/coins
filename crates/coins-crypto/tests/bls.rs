use coins_crypto::{aggregate, sign, verify, verify_aggregate, SecretKey, G1};

#[test]
fn sign_and_verify() {
    let sk = SecretKey::random();
    let pk = G1(sk.public_key());
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
        pairs.push((G1(sk.public_key()), msg));
    }

    let sigma = aggregate(sigs.iter());
    // convert to iterator over (&G1, &[u8])
    let check_iter = pairs
        .iter()
        .map(|(pk, m)| (pk, m.as_bytes()));

    assert!(verify_aggregate(check_iter, &sigma));
}

#[test]
fn aggregate_same_signer_different_messages() {
    // Test aggregating signatures from the same signer on different messages
    // This is what happens when one account sends multiple transactions

    let sk = SecretKey::random();
    let pk = G1(sk.public_key());

    let msg1 = b"transaction 1";
    let msg2 = b"transaction 2";

    let sig1 = sign(&sk, msg1);
    let sig2 = sign(&sk, msg2);

    // Aggregate the two signatures
    let sigma = aggregate([&sig1, &sig2]);

    // Verify aggregate
    let pairs = vec![(&pk, msg1.as_slice()), (&pk, msg2.as_slice())];
    assert!(verify_aggregate(pairs.into_iter(), &sigma),
            "Aggregate verification should work for same signer with different messages");
} 