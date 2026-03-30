const {
    Connection,
    Keypair,
    SystemProgram,
    Transaction,
    TransactionInstruction,
    PublicKey,
    sendAndConfirmTransaction,
    clusterApiUrl,
} = require('@solana/web3.js');
const fs = require('fs');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
    const PROGRAM_ID = new PublicKey('DjpYyLcJBS6HGMu8ZYWgvwUYZNwkV5Bg3pQZhx3rAaJu');
    const connection = new Connection(clusterApiUrl('devnet'), 'confirmed');
    
    // Load wallet
    let secretKeyString = fs.readFileSync(process.env.HOME + '/.config/solana/id.json', 'utf8');
    const creator = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(secretKeyString)));
    // We'll use the same wallet as donor for simplicity.
    const donor = creator; 

    console.log("🪪 Menggunakan Wallet Utama:", creator.publicKey.toBase58());

    // --- SKENARIO 1: WITHDRAW (Campaign Sukses) ---
    console.log("\n==============================================");
    console.log("🎬 SKENARIO 1: WITHDRAW (Campaign Mencapai Goal)");
    console.log("==============================================");
    
    let wCampaignKey = Keypair.generate();
    let wGoal = BigInt(0.1 * 1e9); 
    // Deadline is 10 seconds from now
    let wDeadline = BigInt(Math.floor(Date.now() / 1000) + 10); 

    // 1a. Create Campaign
    const createDataW = Buffer.alloc(17);
    createDataW.writeUInt8(0, 0);       
    createDataW.writeBigUInt64LE(wGoal, 1);
    createDataW.writeBigInt64LE(wDeadline, 9); 

    let rentExempt = await connection.getMinimumBalanceForRentExemption(57);
    const txCreateW = new Transaction().add(
        SystemProgram.createAccount({
            fromPubkey: creator.publicKey,
            newAccountPubkey: wCampaignKey.publicKey,
            lamports: rentExempt,
            space: 57,
            programId: PROGRAM_ID
        }),
        new TransactionInstruction({
            programId: PROGRAM_ID,
            keys: [
                { pubkey: wCampaignKey.publicKey, isSigner: false, isWritable: true },
                { pubkey: creator.publicKey, isSigner: true, isWritable: false },
                { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
            ],
            data: createDataW
        })
    );
    let sigCreateW = await sendAndConfirmTransaction(connection, txCreateW, [creator, wCampaignKey]);
    console.log("✅ [Create Campaign] Sukses. Target=0.1 SOL, Deadline=+10s\n   Tx:", sigCreateW);

    // 1b. Contribute
    const [vaultW] = PublicKey.findProgramAddressSync([Buffer.from("vault"), wCampaignKey.publicKey.toBuffer()], PROGRAM_ID);
    const [contribW] = PublicKey.findProgramAddressSync([Buffer.from("contribution"), wCampaignKey.publicKey.toBuffer(), donor.publicKey.toBuffer()], PROGRAM_ID);
    const wAmount = BigInt(0.1 * 1e9); 

    const contribDataW = Buffer.alloc(9);
    contribDataW.writeUInt8(1, 0);
    contribDataW.writeBigUInt64LE(wAmount, 1);

    const txContribW = new Transaction().add(
        new TransactionInstruction({
            programId: PROGRAM_ID,
            keys: [
                { pubkey: wCampaignKey.publicKey, isSigner: false, isWritable: true },  
                { pubkey: donor.publicKey, isSigner: true, isWritable: true },         
                { pubkey: vaultW, isSigner: false, isWritable: true },                   
                { pubkey: contribW, isSigner: false, isWritable: true },            
                { pubkey: SystemProgram.programId, isSigner: false, isWritable: false }
            ],
            data: contribDataW
        })
    );
    let sigContribW = await sendAndConfirmTransaction(connection, txContribW, [donor]);
    console.log("✅ [Contribute] Sukses 0.1 SOL (Goal Tecapai!).\n   Tx:", sigContribW);

    console.log("⏳ Menunggu melewati deadline (10+ detik)...");
    await sleep(12000); 

    // 1c. Withdraw
    const txWithdraw = new Transaction().add(
        new TransactionInstruction({
            programId: PROGRAM_ID,
            keys: [
                { pubkey: wCampaignKey.publicKey, isSigner: false, isWritable: true },  
                { pubkey: creator.publicKey, isSigner: true, isWritable: true },         
                { pubkey: vaultW, isSigner: false, isWritable: true },                               
                { pubkey: SystemProgram.programId, isSigner: false, isWritable: false }
            ],
            data: Buffer.from([2]) // discriminator = 2
        })
    );
    let sigWithdraw = await sendAndConfirmTransaction(connection, txWithdraw, [creator]);
    console.log("✅ [Withdraw] Sukses mencairkan dana ke Creator!\n   Tx:", sigWithdraw);


    // --- SKENARIO 2: REFUND (Campaign Gagal) ---
    console.log("\n==============================================");
    console.log("🎬 SKENARIO 2: REFUND (Campaign Gagal Capai Goal)");
    console.log("==============================================");

    let rCampaignKey = Keypair.generate();
    let rGoal = BigInt(1.0 * 1e9); // 1 SOL
    let rDeadline = BigInt(Math.floor(Date.now() / 1000) + 10); 

    // 2a. Create Campaign
    const createDataR = Buffer.alloc(17);
    createDataR.writeUInt8(0, 0);       
    createDataR.writeBigUInt64LE(rGoal, 1);
    createDataR.writeBigInt64LE(rDeadline, 9); 

    const txCreateR = new Transaction().add(
        SystemProgram.createAccount({
            fromPubkey: creator.publicKey,
            newAccountPubkey: rCampaignKey.publicKey,
            lamports: rentExempt,
            space: 57,
            programId: PROGRAM_ID
        }),
        new TransactionInstruction({
            programId: PROGRAM_ID,
            keys: [
                { pubkey: rCampaignKey.publicKey, isSigner: false, isWritable: true },
                { pubkey: creator.publicKey, isSigner: true, isWritable: false },
                { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
            ],
            data: createDataR
        })
    );
    let sigCreateR = await sendAndConfirmTransaction(connection, txCreateR, [creator, rCampaignKey]);
    console.log("✅ [Create Campaign] Sukses. Target=1.0 SOL, Deadline=+10s\n   Tx:", sigCreateR);

    // 2b. Contribute (only 0.05 SOL, fail to meet goal)
    const [vaultR] = PublicKey.findProgramAddressSync([Buffer.from("vault"), rCampaignKey.publicKey.toBuffer()], PROGRAM_ID);
    const [contribR] = PublicKey.findProgramAddressSync([Buffer.from("contribution"), rCampaignKey.publicKey.toBuffer(), donor.publicKey.toBuffer()], PROGRAM_ID);
    const rAmount = BigInt(0.05 * 1e9); 

    const contribDataR = Buffer.alloc(9);
    contribDataR.writeUInt8(1, 0);
    contribDataR.writeBigUInt64LE(rAmount, 1);

    const txContribR = new Transaction().add(
        new TransactionInstruction({
            programId: PROGRAM_ID,
            keys: [
                { pubkey: rCampaignKey.publicKey, isSigner: false, isWritable: true },  
                { pubkey: donor.publicKey, isSigner: true, isWritable: true },         
                { pubkey: vaultR, isSigner: false, isWritable: true },                   
                { pubkey: contribR, isSigner: false, isWritable: true },            
                { pubkey: SystemProgram.programId, isSigner: false, isWritable: false }
            ],
            data: contribDataR
        })
    );
    let sigContribR = await sendAndConfirmTransaction(connection, txContribR, [donor]);
    console.log("✅ [Contribute] Sukses 0.05 SOL (Target Tidak Tecapai!).\n   Tx:", sigContribR);

    console.log("⏳ Menunggu melewati deadline (10+ detik)...");
    await sleep(12000); 

    // 2c. Refund
    const txRefund = new Transaction().add(
        new TransactionInstruction({
            programId: PROGRAM_ID,
            keys: [
                { pubkey: rCampaignKey.publicKey, isSigner: false, isWritable: false },  
                { pubkey: donor.publicKey, isSigner: true, isWritable: true },         
                { pubkey: vaultR, isSigner: false, isWritable: true },                               
                { pubkey: contribR, isSigner: false, isWritable: true },            
                { pubkey: SystemProgram.programId, isSigner: false, isWritable: false }
            ],
            data: Buffer.from([3]) // discriminator = 3
        })
    );
    let sigRefund = await sendAndConfirmTransaction(connection, txRefund, [donor]);
    console.log("✅ [Refund] Sukses! Uang Donatur Telah Dikembalikan.\n   Tx:", sigRefund);

}

main().catch(err => {
    console.error("❌ Terjadi Error:", err);
});
