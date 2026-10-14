// SilverScript to Kaspa Covenant Indexer (KCI) Plugin Generator
import crypto from 'node:crypto';

export function generatePluginCode({
  artifact,
  contractName = null,
  pluginId = null,
  version = null
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

  const code = `// Auto-generated Covenant Indexer Plugin for SilverScript contract: ${chosenName}
// Generated at: ${new Date().toISOString()}
// Template Hash: ${templateHashHex}

export const CONTRACT_NAME = ${JSON.stringify(chosenName)};
export const TEMPLATE_HASH = ${JSON.stringify(templateHashHex)};
export const STATE_OFFSET = ${stateSpan.offset};
export const STATE_LEN = ${stateSpan.len};

export const DISPATCH_TAGS = ${JSON.stringify(tagToMethod, null, 2)};
export const STATE_FIELDS = ${JSON.stringify(runtimeState.fields, null, 2)};

/**
 * Extracts action / method name from transaction witness or signatureScript
 */
export function actionOf(tx) {
  const input = tx?.inputs?.[0];
  const sig = input?.signatureScript || '';
  // Check 4-byte dispatch tag in witness/signatureScript
  if (typeof sig === 'string' && sig.length >= 8) {
    for (const [tag, name] of Object.entries(DISPATCH_TAGS)) {
      if (sig.toLowerCase().includes(tag)) return name;
    }
  }
  // Check witness items if available
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

  // Tracked addresses for event subscriptions (Genesis factory or registry if any)
  const watch = new Set(Array.isArray(options.watchAddresses) ? options.watchAddresses : []);

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

    const covenantId = output.covenant?.covenantId || txid;
    const spk = output.scriptPublicKey;

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
      state: {
        phase: 1,
        contractName: CONTRACT_NAME,
        templateHash: targetTemplateHash,
        step: 0,
        genesisDaa: block?.daa || null
      },
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

    // Check if the covenant continues or terminates
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
    discover: utxo => null,
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
    dispatchTags: tagToMethod
  };
}
