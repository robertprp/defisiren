use alloy_primitives::{keccak256, Address, B256};
use serde::{Deserialize, Serialize};
use siren_lib::{AppResult, Error};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAnalysis {
    pub version: String,
    pub exact_hash: B256,
    pub normalized_hash: B256,
    pub instruction_offsets: Vec<usize>,
    pub selectors: Vec<[u8; 4]>,
    pub embedded_address_candidates: Vec<Address>,
    pub calls: usize,
    pub delegates: usize,
    pub creates: usize,
    pub truncated_push: bool,
    pub delegated_account: Option<Address>,
    pub minimal_clone: Option<Address>,
}
/// Similarity v1 zeros PUSH operands only. It is never an exact malicious-code match.
pub fn analyze(code: &[u8], max_bytes: usize) -> AppResult<CodeAnalysis> {
    if code.len() > max_bytes {
        return Err(Error::Limit("bytecode".into()));
    }
    let mut out = CodeAnalysis {
        version: "evm-instructions/1".into(),
        exact_hash: keccak256(code),
        normalized_hash: B256::ZERO,
        instruction_offsets: vec![],
        selectors: vec![],
        embedded_address_candidates: vec![],
        calls: 0,
        delegates: 0,
        creates: 0,
        truncated_push: false,
        delegated_account: None,
        minimal_clone: None,
    };
    if code.len() == 23 && code[..3] == [0xef, 0x01, 0x00] {
        out.delegated_account = Some(Address::from_slice(&code[3..]));
    }
    if code.len() == 45
        && code[..10] == [0x36, 0x3d, 0x3d, 0x37, 0x3d, 0x3d, 0x3d, 0x36, 0x3d, 0x73]
        && code[30..]
            == [
                0x5a, 0xf4, 0x3d, 0x82, 0x80, 0x3e, 0x90, 0x3d, 0x91, 0x60, 0x2b, 0x57, 0xfd, 0x5b,
                0xf3,
            ]
    {
        out.minimal_clone = Some(Address::from_slice(&code[10..30]));
    }
    let mut normalized = code.to_vec();
    let mut pc = 0;
    while pc < code.len() {
        out.instruction_offsets.push(pc);
        let op = code[pc];
        pc += 1;
        match op {
            0xf1 | 0xf2 | 0xfa => out.calls += 1,
            0xf4 => out.delegates += 1,
            0xf0 | 0xf5 => out.creates += 1,
            _ => {}
        }
        if (0x60..=0x7f).contains(&op) {
            let n = (op - 0x5f) as usize;
            let end = (pc + n).min(code.len());
            if end - pc != n {
                out.truncated_push = true;
            }
            if n == 4 && end - pc == 4 {
                out.selectors.push(
                    code[pc..end]
                        .try_into()
                        .map_err(|_| Error::Invalid("selector".into()))?,
                );
            }
            if n == 20 && end - pc == 20 {
                out.embedded_address_candidates
                    .push(Address::from_slice(&code[pc..end]));
            }
            normalized[pc..end].fill(0);
            pc = end;
        }
    }
    out.normalized_hash = keccak256(normalized);
    Ok(out)
}
