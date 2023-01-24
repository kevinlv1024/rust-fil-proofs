use std::marker::PhantomData;

use bellperson::{gadgets::num::AllocatedNum, Circuit, ConstraintSystem, SynthesisError};
use ff::{Field, PrimeField};
use filecoin_hashers::{poseidon::PoseidonHasher, Hasher, PoseidonMDArity, R1CSHasher};
use generic_array::typenum::Unsigned;
use storage_proofs_core::{
    compound_proof::CircuitComponent,
    gadgets::{constraint, por::PoRCircuit, variables::Root},
    merkle::MerkleTreeTrait,
    por,
    util::NODE_SIZE,
};

use crate::election::{self as vanilla, generate_leaf_challenge};

/// This is the `ElectionPoSt` circuit.
pub struct ElectionPoStCircuit<Tree>
where
    Tree: MerkleTreeTrait,
    Tree::Hasher: R1CSHasher,
{
    pub comm_r: Option<Tree::Field>,
    pub comm_c: Option<Tree::Field>,
    pub comm_r_last: Option<Tree::Field>,
    pub leafs: Vec<Option<Tree::Field>>,
    #[allow(clippy::type_complexity)]
    pub paths: Vec<Vec<(Vec<Option<Tree::Field>>, Option<usize>)>>,
    pub partial_ticket: Option<Tree::Field>,
    pub randomness: Option<Tree::Field>,
    pub prover_id: Option<Tree::Field>,
    pub sector_id: Option<Tree::Field>,
    pub _t: PhantomData<Tree>,
}

#[derive(Clone, Default)]
pub struct ComponentPrivateInputs {}

impl<Tree> CircuitComponent for ElectionPoStCircuit<Tree>
where
    Tree: MerkleTreeTrait,
    Tree::Hasher: R1CSHasher,
{
    type ComponentPrivateInputs = ComponentPrivateInputs;
}

impl<Tree> Circuit<Tree::Field> for ElectionPoStCircuit<Tree>
where
    Tree: MerkleTreeTrait,
    Tree::Hasher: R1CSHasher,
    PoseidonHasher<Tree::Field>: R1CSHasher<Field = Tree::Field>,
{
    fn synthesize<CS>(self, cs: &mut CS) -> Result<(), SynthesisError>
    where
        CS: ConstraintSystem<Tree::Field>,
    {
        let comm_r = self.comm_r;
        let comm_c = self.comm_c;
        let comm_r_last = self.comm_r_last;
        let leafs = self.leafs;
        let paths = self.paths;
        let partial_ticket = self.partial_ticket;
        let randomness = self.randomness;
        let prover_id = self.prover_id;
        let sector_id = self.sector_id;

        assert_eq!(paths.len(), leafs.len());

        // 1. Verify comm_r

        let comm_r_last_num = AllocatedNum::alloc(cs.namespace(|| "comm_r_last"), || {
            comm_r_last
                .map(Into::into)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let comm_c_num = AllocatedNum::alloc(cs.namespace(|| "comm_c"), || {
            comm_c
                .map(Into::into)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let comm_r_num = AllocatedNum::alloc(cs.namespace(|| "comm_r"), || {
            comm_r
                .map(Into::into)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        comm_r_num.inputize(cs.namespace(|| "comm_r_input"))?;

        // Verify H(Comm_C || comm_r_last) == comm_r
        {
            let hash_num = Tree::Hasher::hash2_circuit(
                cs.namespace(|| "H_comm_c_comm_r_last"),
                &comm_c_num,
                &comm_r_last_num,
            )?;

            // Check actual equality
            constraint::equal(
                cs,
                || "enforce_comm_c_comm_r_last_hash_comm_r",
                &comm_r_num,
                &hash_num,
            );
        }

        // 2. Verify Inclusion Paths
        for (i, (leaf, path)) in leafs.iter().zip(paths.iter()).enumerate() {
            PoRCircuit::<Tree>::synthesize(
                cs.namespace(|| format!("challenge_inclusion{}", i)),
                Root::Val(*leaf),
                path.clone().into(),
                Root::from_allocated::<CS>(comm_r_last_num.clone()),
                true,
            )?;
        }

        // 3. Verify partial ticket

        // randomness
        let randomness_num = AllocatedNum::alloc(cs.namespace(|| "randomness"), || {
            randomness
                .map(Into::into)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        // prover_id
        let prover_id_num = AllocatedNum::alloc(cs.namespace(|| "prover_id"), || {
            prover_id
                .map(Into::into)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        // sector_id
        let sector_id_num = AllocatedNum::alloc(cs.namespace(|| "sector_id"), || {
            sector_id
                .map(Into::into)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let mut partial_ticket_nums = vec![randomness_num, prover_id_num, sector_id_num];
        for (i, leaf) in leafs.iter().enumerate() {
            let leaf_num = AllocatedNum::alloc(cs.namespace(|| format!("leaf_{}", i)), || {
                leaf.map(Into::into)
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;
            partial_ticket_nums.push(leaf_num);
        }

        // pad to a multiple of md arity
        let arity = PoseidonMDArity::to_usize();
        while partial_ticket_nums.len() % arity != 0 {
            partial_ticket_nums.push(AllocatedNum::alloc(
                cs.namespace(|| format!("padding_{}", partial_ticket_nums.len())),
                || Ok(Tree::Field::zero()),
            )?);
        }

        // hash it
        let partial_ticket_num = PoseidonHasher::<Tree::Field>::hash_md_circuit::<_>(
            &mut cs.namespace(|| "partial_ticket_hash"),
            &partial_ticket_nums,
        )?;

        // allocate expected input
        let expected_partial_ticket_num =
            AllocatedNum::alloc(cs.namespace(|| "partial_ticket"), || {
                partial_ticket
                    .map(Into::into)
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;

        expected_partial_ticket_num.inputize(cs.namespace(|| "partial_ticket_input"))?;

        // check equality
        constraint::equal(
            cs,
            || "enforce partial_ticket is correct",
            &partial_ticket_num,
            &expected_partial_ticket_num,
        );

        Ok(())
    }
}

impl<Tree> ElectionPoStCircuit<Tree>
where
    Tree: MerkleTreeTrait,
    Tree::Hasher: R1CSHasher,
{
    pub fn generate_public_inputs(
        pub_params: &vanilla::PublicParams,
        pub_inputs: &vanilla::PublicInputs<<Tree::Hasher as Hasher>::Domain>,
    ) -> storage_proofs_core::error::Result<Vec<Tree::Field>> {
        let mut inputs = Vec::new();

        let por_pub_params = por::PublicParams {
            leaves: (pub_params.sector_size as usize / NODE_SIZE),
            private: true,
        };

        // 1. Inputs for verifying comm_r = H(comm_c || comm_r_last)

        inputs.push(pub_inputs.comm_r.into());

        // 2. Inputs for verifying inclusion paths

        for n in 0..pub_params.challenge_count {
            let challenged_leaf_start = generate_leaf_challenge(
                pub_params,
                pub_inputs.randomness,
                pub_inputs.sector_challenge_index,
                n as u64,
            )?;
            for i in 0..pub_params.challenged_nodes {
                let por_pub_inputs = por::PublicInputs {
                    commitment: None,
                    challenge: challenged_leaf_start as usize + i,
                };
                let por_inputs =
                    PoRCircuit::<Tree>::generate_public_inputs(&por_pub_params, &por_pub_inputs)?;

                inputs.extend(por_inputs);
            }
        }

        // 3. Inputs for verifying partial_ticket generation
        let partial_ticket = {
            let mut repr = <Tree::Field as PrimeField>::Repr::default();
            repr.as_mut().copy_from_slice(&pub_inputs.partial_ticket);
            Tree::Field::from_repr_vartime(repr).expect("from_repr failure")
        };
        inputs.push(partial_ticket);

        Ok(inputs)
    }
}
