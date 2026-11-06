// Equivalent of lowki-groth16: snarkjs groth16 (fullProve + verify).
import { groth16 } from "snarkjs";
export const fullProve = groth16.fullProve;
export const verify = groth16.verify;
