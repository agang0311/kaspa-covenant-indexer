// SilverScript (silverc) Compiler Wrapper for Kaspa Covenant Indexer (KCI)
import {execFile} from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';

export function findSilverc(customPath = null) {
  const candidates = [
    customPath,
    process.env.SILVERC_PATH,
    path.resolve(process.cwd(), 'bin/silverc'),
    path.resolve(process.cwd(), '../bin/silverc'),
    '/opt/kaspa-covenant-indexer/bin/silverc',
    '/opt/kaswin-event-indexer/bin/silverc'
  ].filter(Boolean);

  for (const c of candidates) {
    try {
      if (fs.existsSync(c)) {
        fs.accessSync(c, fs.constants.X_OK);
        return c;
      }
    } catch {}
  }
  return null;
}

export function normalizeConstructorArgs(args) {
  if (!args || !Array.isArray(args) || !args.length) return [];
  return args.map(item => {
    if (item && typeof item === 'object' && item.kind && item.value !== undefined) {
      return item; // already in ArtifactValue format
    }
    if (typeof item === 'number' || typeof item === 'bigint') {
      return {kind: 'int', value: Number(item)};
    }
    if (typeof item === 'boolean') {
      return {kind: 'bool', value: item};
    }
    if (typeof item === 'string') {
      // Check if hex bytes
      if (/^0x[0-9a-fA-F]+$/.test(item)) {
        const hex = item.slice(2);
        const buf = Buffer.from(hex, 'hex');
        return {kind: 'bytes', value: Array.from(buf)};
      }
      // If integer string
      if (/^-?[0-9]+$/.test(item)) {
        return {kind: 'int', value: Number(item)};
      }
      return {kind: 'text', value: item};
    }
    return {kind: 'text', value: String(item)};
  });
}

export async function compileSilverScript({
  source,
  filename = 'contract.sil',
  constructorArgs = null,
  astOnly = false,
  silvercPath = null
}) {
  if (!source || typeof source !== 'string' || !source.trim()) {
    return {ok: false, error: 'EMPTY_SOURCE', detail: 'Source code cannot be empty'};
  }
  if (source.length > 512 * 1024) {
    return {ok: false, error: 'SOURCE_TOO_LARGE', detail: 'Source exceeds 512KB limit'};
  }

  const bin = findSilverc(silvercPath);
  if (!bin) {
    return {
      ok: false,
      error: 'SILVERC_NOT_FOUND',
      detail: 'SilverScript compiler binary (silverc) not found on host. Please ensure references/silverscript-v1.0.0 is built.'
    };
  }

  const tmpId = crypto.randomBytes(8).toString('hex');
  const tmpDir = path.join(os.tmpdir(), 'silverc-' + tmpId);
  fs.mkdirSync(tmpDir, {recursive: true});

  const srcPath = path.join(tmpDir, filename.endsWith('.sil') ? filename : filename + '.sil');
  fs.writeFileSync(srcPath, source, 'utf8');

  const args = [];
  if (astOnly) {
    args.push('--ast-only', '-c', srcPath);
  } else {
    if (constructorArgs && Array.isArray(constructorArgs) && constructorArgs.length > 0) {
      const ctorPath = path.join(tmpDir, 'ctor.json');
      const normalized = normalizeConstructorArgs(constructorArgs);
      fs.writeFileSync(ctorPath, JSON.stringify(normalized), 'utf8');
      args.push('--constructor-args', ctorPath);
    }
    args.push('-c', srcPath);
  }

  try {
    const {stdout, stderr} = await new Promise((resolve, reject) => {
      execFile(bin, args, {timeout: 10000, maxBuffer: 8 * 1024 * 1024}, (err, stdout, stderr) => {
        if (err) {
          err.stdout = stdout;
          err.stderr = stderr;
          reject(err);
        } else {
          resolve({stdout, stderr});
        }
      });
    });

    let resultJson;
    try {
      resultJson = JSON.parse(stdout);
    } catch (e) {
      return {ok: false, error: 'INVALID_COMPILER_OUTPUT', detail: stdout || stderr};
    }

    if (astOnly) {
      const fields = (resultJson.fields || []).map(f => ({
        name: f.name,
        type: f.type_ref?.base || 'unknown'
      }));
      const params = (resultJson.params || []).map(p => ({
        name: p.name,
        type: p.type_ref?.base || 'unknown'
      }));
      const functions = (resultJson.functions || []).map(fn => ({
        name: fn.name,
        params: (fn.params || []).map(p => ({name: p.name, type: p.type_ref?.base})),
        attributes: (fn.attributes || []).map(a => a.name)
      }));

      return {
        ok: true,
        mode: 'ast',
        contractName: resultJson.name || 'Contract',
        fields,
        params,
        functions,
        raw: resultJson
      };
    }

    // Full Compilation Output Processing
    const contractsSummary = [];
    if (resultJson.contracts) {
      for (const [cName, cData] of Object.entries(resultJson.contracts)) {
        const compiled = cData.compiled || {};
        const templateHashBytes = compiled.template_hash || [];
        const templateHashHex = Buffer.from(templateHashBytes).toString('hex');
        const bytecodeBytes = compiled.bytecode || [];
        const stateSpan = compiled.state_span || {offset: 0, len: 0};

        const entries = [];
        if (cData.entries) {
          for (const [entryKey, entryVal] of Object.entries(cData.entries)) {
            entries.push({
              key: entryKey,
              dispatchTag: entryVal.dispatch_tag,
              params: entryVal.params || []
            });
          }
        }

        const stateFields = cData.runtime_state?.fields || [];

        contractsSummary.push({
          name: cName,
          sourcePath: cData.source_path,
          templateHashHex,
          bytecodeLength: bytecodeBytes.length,
          stateSpan,
          stateFields,
          entries,
          covDeclToAbi: cData.cov_decl_to_abi || {}
        });
      }
    }

    return {
      ok: true,
      mode: 'compiled',
      schemaVersion: resultJson.schema_version,
      compilerVersion: resultJson.compiler_version,
      contracts: contractsSummary,
      artifact: resultJson
    };

  } catch (err) {
    const errorMsg = (err.stderr || err.stdout || err.message || '').trim();
    return {
      ok: false,
      error: 'COMPILATION_ERROR',
      detail: errorMsg
    };
  } finally {
    try {
      fs.rmSync(tmpDir, {recursive: true, force: true});
    } catch {}
  }
}
