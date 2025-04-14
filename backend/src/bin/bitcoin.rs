use anyhow::Result;
use dotenv::dotenv;
use std::env;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use bitcoincore_rpc::bitcoin::Amount;
use bitcoincore_rpc::jsonrpc::error::RpcError;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let bitcoin_rpc_url = env::var("BITCOIN_RPC_URL").unwrap_or_else(|_| "http://localhost:8332".to_string());
    let bitcoin_rpc_user = env::var("BITCOIN_RPC_USER").unwrap_or_else(|_| "test".to_string());
    let bitcoin_rpc_pass = env::var("BITCOIN_RPC_PASS").unwrap_or_else(|_| "qDDZdeQ5vw9XXFeVnXT4PZ--tGN2xNjjR4nrtyszZx0=".to_string());

    println!("rpc url= {}", bitcoin_rpc_url);
    println!("rpc user= {}", bitcoin_rpc_user);
    println!("rpc pass= {}", bitcoin_rpc_pass);

    dotenv().ok();

    let rpc = Client::new(bitcoin_rpc_url.as_str(),
                          Auth::UserPass(bitcoin_rpc_user.to_string(),
                                         bitcoin_rpc_pass.to_string())).unwrap();

    // Generate new address
    let addr = rpc.get_new_address(None, None).unwrap().assume_checked();

    // Mine blocks to get spendable funds
    rpc.generate_to_address(101, &addr).unwrap();

    // Send coins
    let receiver = rpc.get_new_address(None, None).unwrap().assume_checked();
    let txid = rpc.send_to_address(&receiver, Amount::from_btc(1.0).unwrap(), None, None, None, None, None, None).unwrap();
    println!("Transaction ID: {}", txid);
    // Confirm transaction
    let _ = rpc.generate_to_address(1, &addr).unwrap();
    let best_block_hash = rpc.get_best_block_hash().unwrap();
    println!("best block hash: {}", best_block_hash);
    let pending_transactions = rpc.get_raw_mempool().unwrap().clone();
    pending_transactions.contains(&txid).then(|| {
        println!("Transaction {} is in the mempool", txid);
    }).unwrap_or_else(|| {
        println!("Transaction {} is not in the mempool", txid);
    });
    for tx in pending_transactions {
        println!("pending transaction: {}", tx);
    }

    let tx_info = rpc.get_transaction(&txid, Some(true)).unwrap();
    println!("Transaction details: {:#?}", tx_info);

    let tx_info = rpc.get_transaction(&txid, Some(true)).unwrap();
    println!("Confirmations: {}", tx_info.info.confirmations);
    Ok(())
}