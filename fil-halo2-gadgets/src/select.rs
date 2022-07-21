use std::marker::PhantomData;

use halo2_proofs::{
    arithmetic::FieldExt,
    circuit::{AssignedCell, Layouter},
    plonk::{Advice, Column, ConstraintSystem, Error, Selector},
    poly::Rotation,
};

use crate::{boolean::AssignedBit, utilities::ternary};

#[derive(Clone)]
pub struct SelectConfig<F: FieldExt> {
    bit: Column<Advice>,
    arr: [Column<Advice>; 2],
    out: Column<Advice>,
    s_pick: Selector,
    _f: PhantomData<F>,
}

pub struct SelectChip<F: FieldExt> {
    config: SelectConfig<F>,
}

impl<F: FieldExt> SelectChip<F> {
    pub fn construct(config: SelectConfig<F>) -> Self {
        SelectChip { config }
    }

    // # Side Effects
    //
    // All `advice` will be equality enabled.
    pub fn configure(
        meta: &mut ConstraintSystem<F>,
        advice: [Column<Advice>; 4],
    ) -> SelectConfig<F> {
        for col in advice.iter() {
            meta.enable_equality(*col);
        }

        let bit = advice[0];
        let arr = [advice[1], advice[2]];
        let out = advice[3];

        let s_pick = meta.selector();
        meta.create_gate("pick from pair", |meta| {
            let s_pick = meta.query_selector(s_pick);
            let bit = meta.query_advice(bit, Rotation::cur());
            let x_0 = meta.query_advice(arr[0], Rotation::cur());
            let x_1 = meta.query_advice(arr[1], Rotation::cur());
            let out = meta.query_advice(out, Rotation::cur());
            [s_pick * (out - ternary(bit, x_1, x_0))]
        });

        SelectConfig {
            bit,
            arr,
            out,
            s_pick,
            _f: PhantomData,
        }
    }

    pub fn select(
        &self,
        mut layouter: impl Layouter<F>,
        arr: &[AssignedCell<F, F>],
        // little-endian
        index_bits: &[AssignedBit<F>],
    ) -> Result<AssignedCell<F, F>, Error> {
        // `arr`'s length must be a power of two.
        let num_bits = index_bits.len();
        assert_eq!(arr.len(), 1 << num_bits);

        layouter.assign_region(
            || "select",
            |mut region| {
                let mut offset = 0;
                let mut arr = arr.to_vec();

                // Iterate from the most to the least significant bit.
                for (i, bit) in index_bits.iter().rev().enumerate() {
                    let half = arr.len() / 2;
                    arr = (0..half)
                        .map(|j| {
                            self.config.s_pick.enable(&mut region, offset)?;
                            let bit = bit.copy_advice(
                                || format!("copy bit for pair {} of bit {}", j, i),
                                &mut region,
                                self.config.bit,
                                offset,
                            )?;
                            let x_0 = arr[j].copy_advice(
                                || format!("copy x_0 for pair {} of bit {}", j, i),
                                &mut region,
                                self.config.arr[0],
                                offset,
                            )?;
                            let x_1 = arr[half + j].copy_advice(
                                || format!("copy x_1 for pair {} of bit {}", j, i),
                                &mut region,
                                self.config.arr[1],
                                offset,
                            )?;
                            let out = bit.value().zip(x_0.value().zip(x_1.value())).map(
                                |(bit, (x_0, x_1))| {
                                    if bit.into() {
                                        *x_1
                                    } else {
                                        *x_0
                                    }
                                },
                            );
                            let out = region.assign_advice(
                                || format!("out for pair {} of bit {}", j, i),
                                self.config.out,
                                offset,
                                || out,
                            )?;
                            offset += 1;
                            Ok(out)
                        })
                        .collect::<Result<Vec<AssignedCell<F, F>>, Error>>()?;
                }

                assert_eq!(arr.len(), 1);
                Ok(arr[0].clone())
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use halo2_proofs::{
        circuit::{SimpleFloorPlanner, Value},
        dev::MockProver,
        pasta::Fp,
        plonk::Circuit,
    };

    use crate::{boolean::Bit, AdviceIter};

    #[derive(Clone)]
    struct Config<F: FieldExt> {
        select_config: SelectConfig<F>,
        advice: [Column<Advice>; 4],
    }

    struct SelectCircuit<F: FieldExt> {
        arr: Vec<Value<F>>,
        bits: Vec<Value<bool>>,
        expected: Value<F>,
    }

    impl<F: FieldExt> Circuit<F> for SelectCircuit<F> {
        type Config = Config<F>;
        type FloorPlanner = SimpleFloorPlanner;

        fn without_witnesses(&self) -> Self {
            let len = self.arr.len();
            let num_bits = len.trailing_zeros() as usize;
            SelectCircuit {
                arr: vec![Value::unknown(); len],
                bits: vec![Value::unknown(); num_bits],
                expected: Value::unknown(),
            }
        }

        fn configure(meta: &mut ConstraintSystem<F>) -> Self::Config {
            let advice = [
                meta.advice_column(),
                meta.advice_column(),
                meta.advice_column(),
                meta.advice_column(),
            ];

            let select_config = SelectChip::configure(meta, advice);

            Config {
                select_config,
                advice,
            }
        }

        fn synthesize(
            &self,
            config: Self::Config,
            mut layouter: impl Layouter<F>,
        ) -> Result<(), Error> {
            let Config {
                select_config,
                advice,
            } = config;

            let select_chip = SelectChip::construct(select_config);

            let (arr, bits) = layouter.assign_region(
                || "assign arr and bits",
                |mut region| {
                    let mut advice_iter = AdviceIter::from(advice.to_vec());

                    let arr = self
                        .arr
                        .iter()
                        .enumerate()
                        .map(|(i, elem)| {
                            let (offset, col) = advice_iter.next();
                            region.assign_advice(
                                || format!("assign arr[{}]", i),
                                col,
                                offset,
                                || *elem,
                            )
                        })
                        .collect::<Result<Vec<AssignedCell<F, F>>, Error>>()?;

                    let bits = self
                        .bits
                        .iter()
                        .enumerate()
                        .map(|(i, bit)| {
                            let (offset, col) = advice_iter.next();
                            region.assign_advice(
                                || format!("assign bit_{}", i),
                                col,
                                offset,
                                || bit.map(Bit),
                            )
                        })
                        .collect::<Result<Vec<AssignedBit<F>>, Error>>()?;

                    Ok((arr, bits))
                },
            )?;

            select_chip
                .select(layouter.namespace(|| "select"), &arr, &bits)?
                .value()
                .zip(self.expected.as_ref())
                .map(|(out, expected)| assert_eq!(out, expected));

            Ok(())
        }
    }

    #[test]
    fn test_select_chip() {
        for len in [2usize, 4, 8, 64] {
            let bit_len = len.trailing_zeros() as usize;
            let arr: Vec<Value<Fp>> = (0..len as u64).map(|i| Value::known(Fp::from(i))).collect();
            for index in 0..len {
                let circ = SelectCircuit {
                    arr: arr.clone(),
                    bits: (0..bit_len).map(|i| Value::known(index >> i & 1 == 1)).collect(),
                    expected: arr[index],
                };
                let prover = MockProver::run(7, &circ, vec![]).unwrap();
                assert!(prover.verify().is_ok());
            }
        }
    }
}