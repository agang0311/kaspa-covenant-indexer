// SilverScript to Kaspa Covenant Indexer (KCI) Plugin Generator
// Supports Genesis state invariants, Registry discovery subscriptions, and custom Deposit guards.

export function generatePluginCode({
  artifact,
  contractName = null,
  pluginId = null,
  version = null,
  registryAddresses = [],
  registrationSompi = '5000000',
  requiredDeposit = null,
  initialStateGuards = null,
  allowedCreators = []
}) {
  if (!artifact || !artifact.contracts) {
    throw new Error('INVALID_ARTIFACT: missing contracts in compilation artifact');
  }

  const contracts = Object.entries(artifact.contracts);
  if (!contracts.length) throw new Error('NO_CONTRACTS_FOUND: artifact has no compiled contracts');

  const [chosenName, contractData] = contractName && artifact.contracts[contractName]
    ? [contractName, artifact.contracts[contractName]]
    : contracts[0];

  const compiled = contractData.compiled || {};
  const templateHashBytes = compiled.template_hash || [];
  const templateHashHex = Buffer.from(templateHashBytes).toString('hex');
  const stateSpan = compiled.state_span || {offset: 0, len: 0};
  const runtimeState = contractData.runtime_state || {fields: []};
  const entries = contractData.entries || {};
  const covDeclToAbi = contractData.cov_decl_to_abi || {};

  const id = (pluginId || chosenName.toLowerCase()).replace(/[^a-z0-9_-]/g, '-');
  const ver = version || templateHashHex.slice(0, 16);

  // Dispatch tag mapping: e.g. 'f3489a39' -> 'bump'
  const tagToMethod = {};
  for (const [declName, entryKey] of Object.entries(covDeclToAbi)) {
    const entry = entries[entryKey];
    if (entry && entry.dispatch_tag) {
      tagToMethod[entry.dispatch_tag.toLowerCase()] = declName;
    }
  }

  const cleanRegAddrs = Array.isArray(registryAddresses) ? registryAddresses.filter(Boolean) : [];
  const cleanCreators = Array.isArray(allowedCreators) ? allowedCreators.filter(Boolean) : [];
  const cleanGuards = initialStateGuards && typeof initialStateGuards === 'object' ? initialStateGuards : null;
  const cleanDeposit = requiredDeposit && /^[0-9]+$/.test(String(requiredDeposit)) ? String(requiredDeposit) : null;
  const cleanRegSompi = registrationSompi && /^[0-9]+$/.test(String(registrationSompi)) ? String(registrationSompi) : '0';

  const code = `// Auto-generated Covenant Indexer Plugin for SilverScript contract: ${chosenName}
// Generated at: ${new Date().toISOString()}
// Template Hash: ${templateHashHex}

export const CONTRACT_NAME = ${JSON.stringify(chosenName)};
export const TEMPLATE_HASH = ${JSON.stringify(templateHashHex)};
export const STATE_OFFSET = ${stateSpan.offset};
export const STATE_LEN = ${stateSpan.len};

export const DISPATCH_TAGS = ${JSON.stringify(tagToMethod, null, 2)};
export const STATE_FIELDS = ${JSON.stringify(runtimeState.fields, null, 2)};

export const DEFAULT_REGISTRY_ADDRESSES = ${JSON.stringify(cleanRegAddrs)};
export const DEFAULT_REGISTRATION_SOMPI = ${JSON.stringify(cleanRegSompi)};
export const REQUIRED_DEPOSIT_SOMPI = ${JSON.stringify(cleanDeposit)};
export const INITIAL_STATE_GUARDS = ${JSON.stringify(cleanGuards, null, 2)};
export const ALLOWED_CREATORS = ${JSON.stringify(cleanCreators)};

/**
 * Extracts action / method name from transaction witness or signatureScript
 */
export function actionOf(tx) {
  const input = tx?.inputs?.[0];
  const sig = input?.signatureScript || '';
  if (typeof sig === 'string' && sig.length >= 8) {
    for (const [tag, name] of Object.entries(DISPATCH_TAGS)) {
      if (sig.toLowerCase().includes(tag)) return name;
    }
  }
  const witness = input?.witness;
  if (Array.isArray(witness)) {
    for (const item of witness) {
      const hex = typeof item === 'string' ? item.toLowerCase() : '';
      for (const [tag, name] of Object.entries(DISPATCH_TAGS)) {
        if (hex.includes(tag)) return name;
      }
    }
  }
  return 'STEP';
}

/**
 * Validates and instantiates the plugin interface required by KCI ContractRegistry
 */
export async function create(options = {}, context = {}) {
  const pluginId = options.id || ${JSON.stringify(id)};
  const pluginVer = options.version || ${JSON.stringify(ver)};
  const targetTemplateHash = options.templateHash || TEMPLATE_HASH;

  // 1. Watched Registry Addresses for zero-scan event-driven discovery
  const watchList = options.registryAddresses || DEFAULT_REGISTRY_ADDRESSES;
  const watch = new Set(Array.isArray(watchList) ? watchList : []);
  const regFee = BigInt(options.registrationSompi || DEFAULT_REGISTRATION_SOMPI || '0');

  // 2. Genesis State & Deposit Guards
  const reqDeposit = options.requiredDeposit !== undefined && options.requiredDeposit !== null
    ? (options.requiredDeposit === '0' || options.requiredDeposit === null ? null : BigInt(options.requiredDeposit))
    : (REQUIRED_DEPOSIT_SOMPI ? BigInt(REQUIRED_DEPOSIT_SOMPI) : null);

  const stateGuards = options.initialStateGuards || INITIAL_STATE_GUARDS;
  const creatorWhitelist = new Set(options.allowedCreators || ALLOWED_CREATORS || []);

  /**
   * Fast pre-filter before accepted-row hydration
   */
  const inspectGenesis = tx => {
    const output = tx?.outputs?.[0];
    if (!output) return {match: false, reason: 'NO_OUTPUT_0'};

    // Match covenant binding or script template
    const spk = typeof output.scriptPublicKey === 'string'
      ? output.scriptPublicKey.toLowerCase()
      : (output.scriptPublicKey?.script || '').toLowerCase();

    // Check if output script contains the 32-byte template hash
    if (!spk.includes(targetTemplateHash.toLowerCase())) {
      return {match: false, reason: 'TEMPLATE_HASH_MISMATCH'};
    }

    // Verify Deposit Amount if required by guard
    if (reqDeposit !== null && BigInt(output.value ?? 0) !== reqDeposit) {
      return {match: false, reason: 'GENESIS_DEPOSIT_MISMATCH'};
    }

    // Verify Creator Whitelist if configured
    if (creatorWhitelist.size > 0) {
      const senderAddr = tx.inputs?.[0]?.verboseData?.utxoEntry?.scriptPublicKeyAddress;
      if (!senderAddr || !creatorWhitelist.has(senderAddr)) {
        return {match: false, reason: 'CREATOR_NOT_WHITELISTED'};
      }
    }

    return {match: true};
  };

  /**
   * Constructs initial Round state machine object upon on-chain Genesis acceptance
   */
  const genesis = (tx, block) => {
    const output = tx?.outputs?.[0];
    if (!output) return null;
    const txid = tx?.verboseData?.transactionId || tx?.transactionId || tx?.id;
    if (!txid) return null;

    const insp = inspectGenesis(tx);
    if (!insp.match) return null;

    const covenantId = output.covenant?.covenantId || txid;
    const spk = output.scriptPublicKey;

    const initialRoundState = {
      phase: 1,
      contractName: CONTRACT_NAME,
      templateHash: targetTemplateHash,
      step: 0,
      genesisDaa: block?.daa || null
    };

    // Verify Initial State Guards if specified
    if (stateGuards && typeof stateGuards === 'object') {
      for (const [k, expected] of Object.entries(stateGuards)) {
        if (initialRoundState[k] !== undefined && String(initialRoundState[k]) !== String(expected)) {
          return null; // Initial state invariant violated
        }
      }
    }

    return {
      id: txid,
      cid: covenantId,
      contract: pluginId + '@' + pluginVer,
      status: 'LIVE',
      tip: {transactionId: txid, index: 0},
      spk: typeof spk === 'string' ? {version: 0, script: spk} : spk,
      address: output.verboseData?.scriptPublicKeyAddress || null,
      value: String(output.value ?? '0'),
      terminal: null,
      state: initialRoundState,
      updatedAt: Date.now()
    };
  };

  /**
   * Advances the Round state when a successor spend transaction is accepted
   */
  const advance = (round, tx, block) => {
    const txid = tx?.verboseData?.transactionId || tx?.transactionId || tx?.id;
    if (!txid) throw new Error('MISSING_TXID');

    const action = actionOf(tx);
    const output = tx?.outputs?.[0];

    const spk = typeof output?.scriptPublicKey === 'string'
      ? output.scriptPublicKey.toLowerCase()
      : (output?.scriptPublicKey?.script || '').toLowerCase();

    const continues = output && spk.includes(targetTemplateHash.toLowerCase());

    if (!continues) {
      // Terminal spend (funds left the covenant or round settled)
      return {
        ...round,
        tip: null,
        terminal: action === 'CLOSE' ? 'CLOSED' : 'TERMINAL',
        status: 'TERMINAL',
        updatedAt: Date.now()
      };
    }

    // Successor continuation state
    const nextState = {
      ...(round.state || {}),
      step: ((round.state?.step || 0) + 1),
      lastAction: action,
      lastDaa: block?.daa || null
    };

    return {
      ...round,
      tip: {transactionId: txid, index: 0},
      value: String(output.value ?? round.value),
      status: 'LIVE',
      state: nextState,
      updatedAt: Date.now()
    };
  };

  return {
    id: pluginId,
    version: pluginVer,
    profileId: targetTemplateHash,
    watchAddresses: () => [...watch],
    // Event-driven zero-scan discovery via watched registry output
    discover: u => {
      if (!u.address || !watch.has(u.address)) return null;
      if (regFee > 0n && BigInt(u.amount) !== regFee) return null;
      return u.outpoint?.transactionId || null;
    },
    candidateGenesis: tx => inspectGenesis(tx).match,
    inspectGenesis,
    matchesGenesis: tx => inspectGenesis(tx).match,
    genesis,
    advance,
    kindOf: actionOf
  };
}
`;

  return {
    ok: true,
    code,
    contractName: chosenName,
    pluginId: id,
    version: ver,
    filename: `${id}.mjs`,
    templateHashHex,
    stateSpan,
    runtimeState,
    dispatchTags: tagToMethod,
    registryAddresses: cleanRegAddrs,
    registrationSompi: cleanRegSompi,
    requiredDeposit: cleanDeposit,
    initialStateGuards: cleanGuards,
    allowedCreators: cleanCreators
  };
}
