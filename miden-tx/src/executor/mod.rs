use super::{
    AccountCode, AccountId, DataStore, Digest, NoteOrigin, NoteScript, NoteTarget,
    PreparedTransaction, ProgramAst, RecAdviceProvider, TransactionComplier,
    TransactionExecutorError, TransactionWitness,
};

/// The [TransactionExecutor] has the following responsibilities:
/// - Fetch the data required to execute a transaction from the [DataStore].
/// - Compile the transaction into a [Program] using the [TransactionComplier].
/// - Execute the transaction [Program] and create a [TransactionWitness] using the [RecAdviceProvider].
///
/// The [TransactionExecutor] is generic over the [DataStore] which allows it to be used
/// with different data backend implementations.
///
/// The [TransactionExecutor] `execute_transaction` method is the main entry point for the executor
/// and produces a [TransactionWitness] for the transaction. The [TransactionWitness] is then used to by the prover
/// to generate a proof for the transaction.
pub struct TransactionExecutor<D: DataStore> {
    compiler: TransactionComplier,
    data_store: D,
}

impl<D: DataStore> TransactionExecutor<D> {
    // CONSTRUCTOR
    // --------------------------------------------------------------------------------------------
    /// Creates a new [TransactionExecutor] instance with the specified [DataStore].
    pub fn new(data_store: D) -> Self {
        let compiler = TransactionComplier::new();
        Self {
            compiler,
            data_store,
        }
    }

    // MODIFIERS
    // --------------------------------------------------------------------------------------------
    /// Fetches the account code [ModuleAst] from the [DataStore] and loads it into the compiler.
    /// Returns the account code [AccountCode] that is compiled.
    ///
    /// Errors:
    /// - If the account code cannot be fetched from the [DataStore].
    /// - If the account code fails to be loaded into the compiler.
    pub fn load_account(
        &mut self,
        account_id: AccountId,
    ) -> Result<AccountCode, TransactionExecutorError> {
        let account_code = self
            .data_store
            .get_account_code(account_id)
            .map_err(TransactionExecutorError::FetchAccountCodeFailed)?;
        self.compiler
            .load_account(account_id, account_code)
            .map_err(TransactionExecutorError::LoadAccountFailed)
    }

    /// Loads the provided account interface (vector of procedure digests) into the the compiler.
    /// Returns the old account interface if it previously existed.
    pub fn load_account_interface(
        &mut self,
        account_id: AccountId,
        procedures: Vec<Digest>,
    ) -> Option<Vec<Digest>> {
        self.compiler.load_account_interface(account_id, procedures)
    }

    /// Compiles the provided program into the [NoteScript] and checks (to the extent possible)
    /// if a note could be executed against all accounts with the specified interfaces.
    pub fn compile_note_script(
        &mut self,
        note_script_ast: ProgramAst,
        target_account_procs: Vec<NoteTarget>,
    ) -> Result<NoteScript, TransactionExecutorError> {
        self.compiler
            .compile_note_script(note_script_ast, target_account_procs)
            .map_err(TransactionExecutorError::CompileNoteScriptFailed)
    }

    /// Fetches the data required to execute the transaction from the [DataStore], compiles the
    /// transaction into a [Program] using the [TransactionComplier], and returns a
    /// [PreparedTransaction].
    ///
    /// Errors:
    /// - If required data can not be fetched from the [DataStore].
    /// - If the transaction can not be compiled.
    fn prepare_transaction(
        &mut self,
        account_id: AccountId,
        block_ref: u32,
        note_origins: &[NoteOrigin],
        tx_script: Option<ProgramAst>,
    ) -> Result<PreparedTransaction, TransactionExecutorError> {
        let (account, block_header, block_chain, notes) = self
            .data_store
            .get_transaction_data(account_id, block_ref, note_origins)
            .map_err(TransactionExecutorError::FetchTransactionDataFailed)?;

        let (tx_program, tx_script_root) = self
            .compiler
            .compile_transaction(account_id, &notes, tx_script)
            .map_err(TransactionExecutorError::CompileTransactionError)?;

        Ok(PreparedTransaction::new(
            account,
            block_header,
            block_chain,
            notes,
            tx_script_root,
            tx_program,
        ))
    }

    /// Prepares and executes a transaction specified by the provided arguments and returns a
    /// [TransactionWitness].
    ///
    /// The method first fetches the data required to execute the transaction from the [DataStore]
    /// and compile the transaction into a [Program]. Then it executes the transaction [Program]
    /// and creates a [TransactionWitness] using the [RecAdviceProvider].
    ///
    /// Errors:
    /// - If required data can not be fetched from the [DataStore].
    /// - If the transaction program can not be compiled.
    /// - If the transaction program can not be executed.
    pub fn execute_transaction(
        &mut self,
        account_id: AccountId,
        block_ref: u32,
        note_origins: &[NoteOrigin],
        tx_script: Option<ProgramAst>,
    ) -> Result<TransactionWitness, TransactionExecutorError> {
        let transaction =
            self.prepare_transaction(account_id, block_ref, note_origins, tx_script)?;

        let mut advice_recorder: RecAdviceProvider = transaction.advice_provider_inputs().into();
        let _result = processor::execute(
            transaction.tx_program(),
            transaction.stack_inputs(),
            &mut advice_recorder,
        )
        .map_err(TransactionExecutorError::ExecuteTransactionProgramFailed)?;
        let advice_proof = advice_recorder.into_proof();

        Ok(TransactionWitness::new(
            transaction.account().id(),
            transaction.account().hash(),
            transaction.block_header().hash(),
            transaction.consumed_notes().commitment(),
            transaction.tx_script_root(),
            transaction.tx_program().clone(),
            advice_proof,
        ))
    }
}
