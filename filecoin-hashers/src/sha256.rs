use std::cmp::Ordering;
use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;

use bellperson::{
    gadgets::{boolean::Boolean, multipack, num::AllocatedNum, sha256::sha256 as sha256_circuit},
    ConstraintSystem, SynthesisError,
};
use blstrs::Scalar as Fr;
use ff::{PrimeField, PrimeFieldBits};
#[cfg(feature = "halo2")]
use halo2_proofs::pasta::{Fp, Fq};
use merkletree::{
    hash::{Algorithm, Hashable},
    merkle::Element,
};
#[cfg(feature = "nova")]
use pasta_curves::{Fp, Fq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::{Domain, HashFunction, Hasher, R1CSHasher};

#[derive(Copy, Clone, Default)]
pub struct Sha256Domain<F> {
    pub state: [u8; 32],
    _f: PhantomData<F>,
}

impl<F> Debug for Sha256Domain<F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256Domain({})", hex::encode(&self.state))
    }
}

// Can't blanket `impl<F> From<F> for Sha256Domain<F> where F: PrimeField` because it can conflict
// with `impl<F> From<[u8; 32]> for Sha256Domain<F>`, i.e. `[u8; 32]` is an external type which may
// already implement the external trait `PrimeField`, which causes a "conflicting implementation"
// compiler error.
impl From<Fr> for Sha256Domain<Fr> {
    fn from(f: Fr) -> Self {
        Sha256Domain {
            state: f.to_repr(),
            _f: PhantomData,
        }
    }
}
#[cfg(any(feature = "nova", feature = "halo2"))]
impl From<Fp> for Sha256Domain<Fp> {
    fn from(f: Fp) -> Self {
        Sha256Domain {
            state: f.to_repr(),
            _f: PhantomData,
        }
    }
}
#[cfg(any(feature = "nova", feature = "halo2"))]
impl From<Fq> for Sha256Domain<Fq> {
    fn from(f: Fq) -> Self {
        Sha256Domain {
            state: f.to_repr(),
            _f: PhantomData,
        }
    }
}

#[allow(clippy::from_over_into)]
impl Into<Fr> for Sha256Domain<Fr> {
    fn into(self) -> Fr {
        Fr::from_repr_vartime(self.state).expect("from_repr failure")
    }
}
#[cfg(any(feature = "nova", feature = "halo2"))]
#[allow(clippy::from_over_into)]
impl Into<Fp> for Sha256Domain<Fp> {
    fn into(self) -> Fp {
        Fp::from_repr_vartime(self.state).expect("from_repr failure")
    }
}
#[cfg(any(feature = "nova", feature = "halo2"))]
#[allow(clippy::from_over_into)]
impl Into<Fq> for Sha256Domain<Fq> {
    fn into(self) -> Fq {
        Fq::from_repr_vartime(self.state).expect("from_repr failure")
    }
}

impl<F> From<[u8; 32]> for Sha256Domain<F> {
    fn from(bytes: [u8; 32]) -> Self {
        Sha256Domain {
            state: bytes,
            _f: PhantomData,
        }
    }
}

#[allow(clippy::from_over_into)]
impl<F> Into<[u8; 32]> for Sha256Domain<F> {
    fn into(self) -> [u8; 32] {
        self.state
    }
}

impl<F> AsRef<[u8]> for Sha256Domain<F> {
    fn as_ref(&self) -> &[u8] {
        &self.state
    }
}

impl<F> AsRef<Self> for Sha256Domain<F> {
    fn as_ref(&self) -> &Self {
        self
    }
}

// Implement comparison traits by hand because we have not bound `F` to have those traits.
impl<F> PartialEq for Sha256Domain<F> {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
    }
}

impl<F> Eq for Sha256Domain<F> {}

impl<F> PartialOrd for Sha256Domain<F> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.state.partial_cmp(&other.state)
    }
}

impl<F> Ord for Sha256Domain<F> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.state.cmp(&other.state)
    }
}

// The trait bound `F: PrimeField` is necessary because `Element` requires that `F` implements
// `Clone + Send + Sync`.
impl<F: PrimeField> Element for Sha256Domain<F> {
    fn byte_len() -> usize {
        32
    }

    fn from_slice(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), Self::byte_len(), "invalid number of bytes");
        let mut state = [0u8; 32];
        state.copy_from_slice(bytes);
        state.into()
    }

    fn copy_to_slice(&self, bytes: &mut [u8]) {
        bytes.copy_from_slice(&self.state);
    }
}

impl<F> std::hash::Hash for Sha256Domain<F> {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        std::hash::Hash::hash(&self.state, hasher);
    }
}

// Implement `serde` traits by hand because we have not bound `F` to have those traits.
impl<F> Serialize for Sha256Domain<F> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.state.serialize(s)
    }
}
impl<'de, F> Deserialize<'de> for Sha256Domain<F> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        <[u8; 32]>::deserialize(d).map(Into::into)
    }
}

impl Domain for Sha256Domain<Fr> {
    type Field = Fr;
}
#[cfg(any(feature = "nova", feature = "halo2"))]
impl Domain for Sha256Domain<Fp> {
    type Field = Fp;
}
#[cfg(any(feature = "nova", feature = "halo2"))]
impl Domain for Sha256Domain<Fq> {
    type Field = Fq;
}

impl<F> Sha256Domain<F> {
    // Strip the last (most-significant) two bits to ensure that we state within the ~256-bit field
    // `F`; note the fields `Fr`, `Fp`, and `Fq` are each 255-bit fields which fully utilize 254
    // bits, i.e. `254 < log2(field_modulus) < 255`.
    pub fn trim_to_fr32(&mut self) {
        self.state[31] &= 0b0011_1111;
    }
}

#[derive(Clone, Debug, Default)]
pub struct Sha256Function<F> {
    hasher: Sha256,
    _f: PhantomData<F>,
}

impl<F> std::hash::Hasher for Sha256Function<F> {
    fn write(&mut self, msg: &[u8]) {
        self.hasher.update(msg);
    }

    fn finish(&self) -> u64 {
        unreachable!("unused by Function -- should never be called");
    }
}

impl<F> Hashable<Sha256Function<F>> for Sha256Domain<F> {
    fn hash(&self, hasher: &mut Sha256Function<F>) {
        <Sha256Function<F> as std::hash::Hasher>::write(hasher, self.as_ref());
    }
}

impl<F> Algorithm<Sha256Domain<F>> for Sha256Function<F>
where
    F: PrimeField,
    Sha256Domain<F>: Domain<Field = F>,
{
    fn hash(&mut self) -> Sha256Domain<F> {
        let mut digest = [0u8; 32];
        digest.copy_from_slice(self.hasher.clone().finalize().as_ref());
        let mut trimmed = Sha256Domain {
            state: digest,
            _f: PhantomData,
        };
        trimmed.trim_to_fr32();
        trimmed
    }

    fn reset(&mut self) {
        self.hasher.reset();
    }

    fn leaf(&mut self, leaf: Sha256Domain<F>) -> Sha256Domain<F> {
        leaf
    }

    fn node(
        &mut self,
        left: Sha256Domain<F>,
        right: Sha256Domain<F>,
        _height: usize,
    ) -> Sha256Domain<F> {
        left.hash(self);
        right.hash(self);
        self.hash()
    }

    fn multi_node(&mut self, parts: &[Sha256Domain<F>], _height: usize) -> Sha256Domain<F> {
        for part in parts {
            part.hash(self);
        }
        self.hash()
    }
}

impl<F> HashFunction<Sha256Domain<F>> for Sha256Function<F>
where
    F: PrimeField,
    Sha256Domain<F>: Domain<Field = F>,
{
    fn hash(data: &[u8]) -> Sha256Domain<F> {
        let mut digest = [0u8; 32];
        digest.copy_from_slice(Sha256::digest(data).as_ref());
        let mut trimmed: Sha256Domain<F> = digest.into();
        trimmed.trim_to_fr32();
        trimmed
    }

    fn hash2(a: &Sha256Domain<F>, b: &Sha256Domain<F>) -> Sha256Domain<F> {
        let mut digest = [0u8; 32];
        let hashed = Sha256::new().chain_update(a).chain_update(b).finalize();
        digest.copy_from_slice(hashed.as_ref());
        let mut trimmed: Sha256Domain<F> = digest.into();
        trimmed.trim_to_fr32();
        trimmed
    }
}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
pub struct Sha256Hasher<F> {
    _f: PhantomData<F>,
}

// TODO (jake): should hashers over different fields have different names?
const HASHER_NAME: &str = "sha256_hasher";

impl Hasher for Sha256Hasher<Fr> {
    type Field = Fr;
    type Domain = Sha256Domain<Self::Field>;
    type Function = Sha256Function<Self::Field>;

    fn name() -> String {
        HASHER_NAME.into()
    }
}
#[cfg(any(feature = "nova", feature = "halo2"))]
impl Hasher for Sha256Hasher<Fp> {
    type Field = Fp;
    type Domain = Sha256Domain<Self::Field>;
    type Function = Sha256Function<Self::Field>;

    fn name() -> String {
        HASHER_NAME.into()
    }
}
#[cfg(any(feature = "nova", feature = "halo2"))]
impl Hasher for Sha256Hasher<Fq> {
    type Field = Fq;
    type Domain = Sha256Domain<Self::Field>;
    type Function = Sha256Function<Self::Field>;

    fn name() -> String {
        HASHER_NAME.into()
    }
}

// Implement r1cs circuits for BLS12-381 and Pasta scalar fields.
impl<F> R1CSHasher for Sha256Hasher<F>
where
    // `PrimeFieldBits` is required because `AllocatedNum.to_bits_le()` is called below.
    F: PrimeFieldBits,
    Self: Hasher<Field = F>,
{
    fn hash_leaf_circuit<CS: ConstraintSystem<F>>(
        mut cs: CS,
        left: &AllocatedNum<F>,
        right: &AllocatedNum<F>,
        height: usize,
    ) -> Result<AllocatedNum<F>, SynthesisError> {
        let left_bits = left.to_bits_le(cs.namespace(|| "left num into bits"))?;
        let right_bits = right.to_bits_le(cs.namespace(|| "right num into bits"))?;

        Self::hash_leaf_bits_circuit(cs, &left_bits, &right_bits, height)
    }

    fn hash_multi_leaf_circuit<Arity, CS: ConstraintSystem<F>>(
        mut cs: CS,
        leaves: &[AllocatedNum<F>],
        _height: usize,
    ) -> Result<AllocatedNum<F>, SynthesisError> {
        let mut bits = Vec::with_capacity(leaves.len() * F::CAPACITY as usize);
        for (i, leaf) in leaves.iter().enumerate() {
            let mut padded = leaf.to_bits_le(cs.namespace(|| format!("{}_num_into_bits", i)))?;
            while padded.len() % 8 != 0 {
                padded.push(Boolean::Constant(false));
            }

            bits.extend(
                padded
                    .chunks_exact(8)
                    .flat_map(|chunk| chunk.iter().rev())
                    .cloned(),
            );
        }
        Self::hash_circuit(cs, &bits)
    }

    fn hash_leaf_bits_circuit<CS: ConstraintSystem<F>>(
        cs: CS,
        left: &[Boolean],
        right: &[Boolean],
        _height: usize,
    ) -> Result<AllocatedNum<F>, SynthesisError> {
        let mut preimage: Vec<Boolean> = vec![];

        let mut left_padded = left.to_vec();
        while left_padded.len() % 8 != 0 {
            left_padded.push(Boolean::Constant(false));
        }

        preimage.extend(
            left_padded
                .chunks_exact(8)
                .flat_map(|chunk| chunk.iter().rev())
                .cloned(),
        );

        let mut right_padded = right.to_vec();
        while right_padded.len() % 8 != 0 {
            right_padded.push(Boolean::Constant(false));
        }

        preimage.extend(
            right_padded
                .chunks_exact(8)
                .flat_map(|chunk| chunk.iter().rev())
                .cloned(),
        );

        Self::hash_circuit(cs, &preimage[..])
    }

    fn hash_circuit<CS: ConstraintSystem<F>>(
        mut cs: CS,
        bits: &[Boolean],
    ) -> Result<AllocatedNum<F>, SynthesisError> {
        let be_bits = sha256_circuit(cs.namespace(|| "hash"), bits)?;
        let le_bits = be_bits
            .chunks(8)
            .flat_map(|chunk| chunk.iter().rev())
            .take(F::CAPACITY as usize)
            .cloned()
            .collect::<Vec<_>>();
        multipack::pack_bits(cs.namespace(|| "pack_le"), &le_bits)
    }

    fn hash2_circuit<CS: ConstraintSystem<F>>(
        mut cs: CS,
        a_num: &AllocatedNum<F>,
        b_num: &AllocatedNum<F>,
    ) -> Result<AllocatedNum<F>, SynthesisError> {
        // Allocate as booleans
        let a = a_num.to_bits_le(cs.namespace(|| "a_bits"))?;
        let b = b_num.to_bits_le(cs.namespace(|| "b_bits"))?;

        let mut preimage: Vec<Boolean> = vec![];

        let mut a_padded = a.to_vec();
        while a_padded.len() % 8 != 0 {
            a_padded.push(Boolean::Constant(false));
        }

        preimage.extend(
            a_padded
                .chunks_exact(8)
                .flat_map(|chunk| chunk.iter().rev())
                .cloned(),
        );

        let mut b_padded = b.to_vec();
        while b_padded.len() % 8 != 0 {
            b_padded.push(Boolean::Constant(false));
        }

        preimage.extend(
            b_padded
                .chunks_exact(8)
                .flat_map(|chunk| chunk.iter().rev())
                .cloned(),
        );

        Self::hash_circuit(cs, &preimage[..])
    }
}

#[cfg(feature = "halo2")]
mod halo2 {
    use super::*;

    use std::convert::TryInto;

    use fil_halo2_gadgets::{
        sha256::{Sha256FieldChip, Sha256FieldConfig},
        ColumnCount,
    };
    use halo2_proofs::{
        arithmetic::FieldExt,
        circuit::{AssignedCell, Layouter},
        plonk::{self, Advice, Column, Fixed},
    };

    use crate::{Halo2Hasher, HashInstructions, PoseidonArity};

    impl<F: FieldExt> HashInstructions<F> for Sha256FieldChip<F> {
        fn hash(
            &self,
            layouter: impl Layouter<F>,
            preimage: &[AssignedCell<F, F>],
        ) -> Result<AssignedCell<F, F>, plonk::Error> {
            self.hash_field_elems(layouter, preimage)
        }
    }

    impl<F, A> Halo2Hasher<A> for Sha256Hasher<F>
    where
        F: FieldExt,
        A: PoseidonArity<F>,
        Self: Hasher<Field = F>,
    {
        type Chip = Sha256FieldChip<F>;
        type Config = Sha256FieldConfig<F>;

        fn load(layouter: &mut impl Layouter<F>, config: &Self::Config) -> Result<(), plonk::Error> {
            Sha256FieldChip::load(layouter, config)
        }

        fn construct(config: Self::Config) -> Self::Chip {
            Sha256FieldChip::construct(config)
        }

        #[allow(clippy::unwrap_used)]
        fn configure(
            meta: &mut plonk::ConstraintSystem<F>,
            advice_eq: &[Column<Advice>],
            _advice_neq: &[Column<Advice>],
            _fixed_eq: &[Column<Fixed>],
            _fixed_neq: &[Column<Fixed>],
        ) -> Self::Config {
            let num_cols = Self::Chip::num_cols();
            assert!(advice_eq.len() >= num_cols.advice_eq);
            let advice = advice_eq[..num_cols.advice_eq].try_into().unwrap();
            Sha256FieldChip::configure(meta, advice)
        }

        #[inline]
        fn transmute_arity<B>(config: Self::Config) -> <Self as Halo2Hasher<B>>::Config
        where
            B: PoseidonArity<Self::Field>,
        {
            config
        }
    }
}

#[cfg(all(test, any(feature = "nova", feature = "halo2")))]
mod tests {
    use super::*;

    use bellperson::util_cs::test_cs::TestConstraintSystem;
    use ff::Field;
    use generic_array::typenum::U0;

    #[test]
    fn test_sha256_vanilla_all_fields() {
        // Test two one-block and two two-block preimages.
        let preimages = [vec![1u8], vec![0, 55, 0, 0], vec![1; 64], vec![1; 100]];
        for preimage in &preimages {
            let digest_fr: [u8; 32] =
                <Sha256Function<Fr> as HashFunction<_>>::hash(preimage).into();
            let digest_fp: [u8; 32] =
                <Sha256Function<Fp> as HashFunction<_>>::hash(preimage).into();
            let digest_fq: [u8; 32] =
                <Sha256Function<Fq> as HashFunction<_>>::hash(preimage).into();
            assert_eq!(digest_fr, digest_fp);
            assert_eq!(digest_fr, digest_fq);
        }
    }

    #[test]
    fn test_sha256_r1cs_circuit_all_fields() {
        // Choose an arbitrary arity type because it is ignored by the sha256 circuit.
        type A = U0;

        let digest_fr: Fr = {
            let mut cs = TestConstraintSystem::new();
            let preimage =
                [AllocatedNum::alloc(&mut cs, || Ok(Fr::one()))
                    .expect("allocation should not fail")];
            Sha256Hasher::<Fr>::hash_multi_leaf_circuit::<A, _>(&mut cs, &preimage, 0)
                .expect("sha256 failed")
                .get_value()
                .expect("digest should be allocated")
        };
        let digest_fp: Fp = {
            let mut cs = TestConstraintSystem::new();
            let preimage =
                [AllocatedNum::alloc(&mut cs, || Ok(Fp::one()))
                    .expect("allocation should not fail")];
            Sha256Hasher::<Fp>::hash_multi_leaf_circuit::<A, _>(&mut cs, &preimage, 0)
                .expect("sha256 failed")
                .get_value()
                .expect("digest should be allocated")
        };
        let digest_fq: Fq = {
            let mut cs = TestConstraintSystem::new();
            let preimage =
                [AllocatedNum::alloc(&mut cs, || Ok(Fq::one()))
                    .expect("allocation should not fail")];
            Sha256Hasher::<Fq>::hash_multi_leaf_circuit::<A, _>(&mut cs, &preimage, 0)
                .expect("sha256 failed")
                .get_value()
                .expect("digest should be allocated")
        };

        for ((byte_1, byte_2), byte_3) in digest_fr
            .to_repr()
            .as_ref()
            .iter()
            .zip(digest_fp.to_repr().as_ref())
            .zip(digest_fq.to_repr().as_ref())
        {
            assert_eq!(byte_1, byte_2);
            assert_eq!(byte_1, byte_3);
        }
    }

    #[cfg(feature = "halo2")]
    #[test]
    #[allow(clippy::unwrap_used)]
    fn test_sha256_groth16_halo2_compat() {
        use fil_halo2_gadgets::AdviceIter;
        use halo2_proofs::{
            arithmetic::FieldExt,
            circuit::{AssignedCell, Layouter, SimpleFloorPlanner, Value},
            dev::MockProver,
            plonk::{Advice, Circuit, Column, ConstraintSystem, Error},
        };

        use crate::{Halo2Hasher, HashInstructions};

        // Choose an arbitrary arity type because it is ignored by the sha256 circuit.
        type A = U0;

        // Halo2 circuit.
        struct Sha256Circuit<F>
        where
            F: FieldExt,
            Sha256Hasher<F>: Hasher<Field = F>,
        {
            preimage: Vec<Value<F>>,
            groth_digest: Fr,
        }

        impl<F> Circuit<F> for Sha256Circuit<F>
        where
            F: FieldExt,
            Sha256Hasher<F>: Hasher<Field = F>,
        {
            type Config = (
                <Sha256Hasher<F> as Halo2Hasher<A>>::Config,
                [Column<Advice>; 9],
            );
            type FloorPlanner = SimpleFloorPlanner;

            fn without_witnesses(&self) -> Self {
                Sha256Circuit {
                    preimage: vec![Value::unknown(); self.preimage.len()],
                    groth_digest: Fr::zero(),
                }
            }

            fn configure(meta: &mut ConstraintSystem<F>) -> Self::Config {
                let advice = [
                    meta.advice_column(),
                    meta.advice_column(),
                    meta.advice_column(),
                    meta.advice_column(),
                    meta.advice_column(),
                    meta.advice_column(),
                    meta.advice_column(),
                    meta.advice_column(),
                    meta.advice_column(),
                ];
                let sha256 =
                    <Sha256Hasher<F> as Halo2Hasher<A>>::configure(meta, &advice, &[], &[], &[]);
                (sha256, advice)
            }

            fn synthesize(
                &self,
                config: Self::Config,
                mut layouter: impl Layouter<F>,
            ) -> Result<(), Error> {
                let (sha256_config, advice) = config;

                <Sha256Hasher<F> as Halo2Hasher<A>>::load(&mut layouter, &sha256_config)?;
                let sha256_chip = <Sha256Hasher<F> as Halo2Hasher<A>>::construct(sha256_config);

                let preimage = layouter.assign_region(
                    || "assign preimage",
                    |mut region| {
                        let mut advice_iter = AdviceIter::from(advice.to_vec());
                        self.preimage
                            .iter()
                            .enumerate()
                            .map(|(i, elem)| {
                                let (offset, col) = advice_iter.next();
                                region.assign_advice(
                                    || format!("preimage elem {}", i),
                                    col,
                                    offset,
                                    || *elem,
                                )
                            })
                            .collect::<Result<Vec<AssignedCell<F, F>>, Error>>()
                    },
                )?;

                let digest =
                    <<Sha256Hasher<F> as Halo2Hasher<A>>::Chip as HashInstructions<F>>::hash(
                        &sha256_chip,
                        layouter,
                        &preimage,
                    )?;

                let digest_repr: Value<Vec<u8>> = digest
                    .value()
                    .map(|field| field.to_repr().as_ref().to_vec());

                let expected_repr: Value<Vec<u8>> =
                    Value::known(self.groth_digest.to_repr().as_ref().to_vec());

                digest_repr
                    .zip(expected_repr)
                    .assert_if_known(|(repr, expected_repr)| repr == expected_repr);

                Ok(())
            }
        }

        // Test one-element preimage.
        {
            let groth_digest: Fr = {
                let mut cs = TestConstraintSystem::new();
                let preimage = [AllocatedNum::alloc(&mut cs, || Ok(Fr::one())).unwrap()];
                Sha256Hasher::<Fr>::hash_multi_leaf_circuit::<A, _>(&mut cs, &preimage, 0)
                    .unwrap()
                    .get_value()
                    .unwrap()
            };

            // Compute Halo2 digest using Pallas field.
            let circ = Sha256Circuit {
                preimage: vec![Value::known(Fp::one())],
                groth_digest,
            };
            let prover = MockProver::run(17, &circ, vec![]).unwrap();
            assert!(prover.verify().is_ok());

            // Compute Halo2 digest using Vesta field.
            let circ = Sha256Circuit {
                preimage: vec![Value::known(Fq::one())],
                groth_digest,
            };
            let prover = MockProver::run(17, &circ, vec![]).unwrap();
            assert!(prover.verify().is_ok());
        }

        // Test two-element preimage.
        {
            let groth_digest: Fr = {
                let mut cs = TestConstraintSystem::new();
                let preimage = [
                    AllocatedNum::alloc(cs.namespace(|| "preimage elem 1"), || Ok(Fr::one()))
                        .unwrap(),
                    AllocatedNum::alloc(cs.namespace(|| "preimage elem 2"), || Ok(Fr::from(55)))
                        .unwrap(),
                ];
                Sha256Hasher::<Fr>::hash_multi_leaf_circuit::<A, _>(&mut cs, &preimage, 0)
                    .unwrap()
                    .get_value()
                    .unwrap()
            };

            // Compute Halo2 digest using Pallas field.
            let circ = Sha256Circuit {
                preimage: vec![Value::known(Fp::one()), Value::known(Fp::from(55))],
                groth_digest,
            };
            let prover = MockProver::run(17, &circ, vec![]).unwrap();
            assert!(prover.verify().is_ok());

            // Compute Halo2 digest using Vesta field.
            let circ = Sha256Circuit {
                preimage: vec![Value::known(Fq::one()), Value::known(Fq::from(55))],
                groth_digest,
            };
            let prover = MockProver::run(17, &circ, vec![]).unwrap();
            assert!(prover.verify().is_ok());
        }
    }
}
