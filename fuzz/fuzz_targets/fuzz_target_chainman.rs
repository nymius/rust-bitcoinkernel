#![no_main]

use std::sync::Once;

use libfuzzer_sys::fuzz_target;

use arbitrary::Arbitrary;

use libbitcoinkernel_sys::{
    disable_logging, Block, BlockManagerOptions, ChainType, ChainstateLoadOptions,
    ChainstateManager, ChainstateManagerOptions, Context, ContextBuilder,
    KernelNotificationInterfaceCallbackHolder,
};

fn create_context(chain_type: ChainType) -> Context {
    ContextBuilder::new()
        .chain_type(chain_type)
        .unwrap()
        .kn_callbacks(Box::new(KernelNotificationInterfaceCallbackHolder {
            kn_block_tip: Box::new(|_state, _block_index| {}),
            kn_header_tip: Box::new(|_state, _height, _timestamp, _presync| {}),
            kn_progress: Box::new(|_title, _progress, _resume_possible| {}),
            kn_warning_set: Box::new(|_warning, _message| {}),
            kn_warning_unset: Box::new(|_warning| {}),
            kn_flush_error: Box::new(|_message| {}),
            kn_fatal_error: Box::new(|_message| {}),
        }))
        .unwrap()
        .build()
        .unwrap()
}

#[derive(Debug, Arbitrary)]
pub enum FuzzChainType {
    MAINNET,
    TESTNET,
    REGTEST,
    SIGNET,
}

impl Into<ChainType> for FuzzChainType {
    fn into(self) -> ChainType {
        match self {
            FuzzChainType::MAINNET => ChainType::MAINNET,
            FuzzChainType::TESTNET => ChainType::TESTNET,
            FuzzChainType::REGTEST => ChainType::REGTEST,
            FuzzChainType::SIGNET => ChainType::SIGNET,
        }
    }
}

#[derive(Debug, Arbitrary)]
pub struct ChainstateManagerInput {
    pub data_dir: String,
    pub chain_type: FuzzChainType,
    pub blocks: Vec<Vec<u8>>,
    pub wipe_block_index: bool,
    pub wipe_chainstate_index: bool,
    pub block_tree_db_in_memory: bool,
    pub chainstate_db_in_memory: bool,
}

static INIT: Once = Once::new();

fuzz_target!(|data: ChainstateManagerInput| {
    INIT.call_once(|| {
        disable_logging();
    });

    let context = create_context(data.chain_type.into());
    // Sanitize the input string by removing dots and slashes
    let sanitized_string: String = data
        .data_dir
        .chars()
        .filter(|c| *c != '.' && *c != '/')
        .take(60)
        .collect();

    let data_dir = format!("/mnt/tmp/kernel/{}", sanitized_string);
    let blocks_dir = format!("{}/blocks", data_dir);
    let chainman_opts = match ChainstateManagerOptions::new(&context, &data_dir) {
        Ok(opts) => opts,
        Err(libbitcoinkernel_sys::KernelError::CStringCreationFailed(_)) => return,
        Err(err) => panic!("this should never happen: {}", err),
    };
    let blockman_opts = BlockManagerOptions::new(&context, &blocks_dir).unwrap();
    let chainman = ChainstateManager::new(chainman_opts, blockman_opts, &context).unwrap();

    match chainman.load_chainstate(
        ChainstateLoadOptions::new()
            .set_reindex(data.wipe_block_index)
            .unwrap()
            .set_wipe_chainstate_db(data.wipe_chainstate_index)
            .unwrap()
            .set_block_tree_db_in_memory(data.block_tree_db_in_memory)
            .unwrap()
            .set_chainstate_db_in_memory(data.chainstate_db_in_memory)
            .unwrap(),
    ) {
        Err(libbitcoinkernel_sys::KernelError::Internal(_)) => {}
        Err(err) => {
            let _ = std::fs::remove_dir_all(data_dir);
            panic!("this should never happen: {}", err);
        }
        _ => {}
    }
    if let Err(err) = chainman.import_blocks() {
        let _ = std::fs::remove_dir_all(data_dir);
        panic!("this should never happen: {}", err);
    }

    for block in data.blocks {
        if let Ok(block) = Block::try_from(block.as_slice()) {
            let _ = chainman.process_block(&block);
        }
    }
    drop(chainman);
    let _ = std::fs::remove_dir_all(data_dir);
});
