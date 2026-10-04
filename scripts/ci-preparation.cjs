// The existing lane's preparation, reused only at the same native build boundary.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const ledger=require('./ci-ledger.cjs');
const owned=require('./ci-owned-command.cjs');
const MANIFEST='agents/runs/ci-preparation/manifest.json';
const BUILD='agents/runs/ci-preparation/build.json';
const COMMAND=['build','-p','hided','--bins','-p','hide-platform','--example','fixture-owner','--message-format=json-render-diagnostics'];
const TARGETS=[['hided','bin'],['hide','bin'],['fixture-owner','example']];
const MAX_FILES=10000, MAX_FILE_BYTES=512*1024*1024, MAX_TOTAL_BYTES=2*1024*1024*1024;
function digest(file) {
  const stat=fs.lstatSync(file);
  if(!stat.isFile() || stat.size>MAX_FILE_BYTES) throw Error('invalid preparation file: '+file);
  const hash=crypto.createHash('sha256'), buffer=Buffer.alloc(64*1024), fd=fs.openSync(file,'r');
  try { let count; while((count=fs.readSync(fd,buffer,0,buffer.length,null))) hash.update(buffer.subarray(0,count)); }
  finally { fs.closeSync(fd); }
  return {size:stat.size,sha256:hash.digest('hex')};
}
function refuseOverrides() {
  // This lane has exactly one supported native build recipe. Cargo accepts
  // profile/target overrides from the environment even when argv is unchanged.
  // Refuse those before both build and reuse instead of labelling them default.
  const overrides=Object.keys(process.env).filter(key=>key.startsWith('CARGO_PROFILE_') || key==='CARGO_BUILD_TARGET' || key==='CARGO_BUILD_RUSTC' || key==='CI_PREPARATION_FEATURES');
  if(overrides.length) throw Error('unsupported preparation build override: '+overrides.sort().join(', '));
}
async function identity(root, command) {
  refuseOverrides();
  // Verification already owns a Bash entrypoint on every runner. Its command
  // lookup executes Windows pnpm command shims as well as native executables;
  // Node's execFile lookup alone cannot start that same installed shim.
  // Pass separate argv through a fixed script, never interpolate shell text.
  const shell=owned.shell();
  command ||= async (name,args)=> (await owned.run(root,shell,['-c','exec "$@"','preparation-tool',name,...args],{subject:path.basename(name)})).stdout.trim();
  const sha=await command('git',['rev-parse','HEAD']);
  await command('git',['diff','--quiet','HEAD','--']);
  if(process.env.GITHUB_SHA && process.env.GITHUB_SHA!==sha) throw Error('preparation checkout differs from tested SHA');
  const shellVersion=await command(shell,['--version']);
  if(process.platform==='win32' && !shellVersion.includes('pc-msys')) throw Error('preparation requires the Actions Git Bash, not WSL');
  return {sha,os:process.platform,arch:process.arch,command:COMMAND,
    shell:{path:shell,version:shellVersion,digest:digest(shell)},
    rust:await command('rustc',['-vV']),node:process.version,pnpm:await command('pnpm',['--version']),
    rustflags:process.env.CARGO_ENCODED_RUSTFLAGS || process.env.RUSTFLAGS || '',
    cargoLock:digest(path.join(root,'Cargo.lock')).sha256,pnpmLock:digest(path.join(root,'pnpm-lock.yaml')).sha256,
    runtime:digest(path.join(root,'contracts/herdr-bundle.json')).sha256};
}
function inventory(root) {
  const files={}; let total=0;
  function visit(relative) {
    const stat=fs.lstatSync(path.join(root,relative));
    if(stat.isDirectory()) for(const name of fs.readdirSync(path.join(root,relative)).sort()) visit(relative+'/'+name);
    else {
      if(Object.keys(files).length>=MAX_FILES) throw Error('preparation file cap exceeded');
      const value=digest(path.join(root,relative)); total+=value.size;
      if(total>MAX_TOTAL_BYTES) throw Error('preparation byte cap exceeded');
      files[relative]=value;
    }
  }
  for(const name of ['hided','hide']) visit('target/debug/'+name+(process.platform==='win32'?'.exe':''));
  visit('target/debug/examples/fixture-owner'+(process.platform==='win32'?'.exe':''));
  for(const directory of ['web/dist','plugins/hcoord/dist']) {
    visit(directory);
    if(!Object.keys(files).some(name=>name.startsWith(directory+'/'))) throw Error('empty preparation output: '+directory);
  }
  return files;
}
async function confirmBoundary(root, before, command) {
  try {
    if(JSON.stringify(await identity(root,command))!==JSON.stringify(before)) throw Error('identity mismatch');
  } catch(cause) { throw new Error('preparation source/toolchain changed during operation',{cause}); }
}
function verify(value, expected, files) {
  if(value.version!==1 || JSON.stringify(value.identity)!==JSON.stringify(expected)) throw Error('preparation source/profile/features/OS/arch/toolchain mismatch');
  if(JSON.stringify(value.files)!==JSON.stringify(files)) throw Error('preparation output inventory or digest mismatch');
}
module.exports={identity,inventory,verify,digest};
async function main() {
  const root=process.cwd(), mode=process.argv[2], file=path.join(root,MANIFEST);
  const buildFile=path.join(root,BUILD);
  const receiptFile=path.join(root,'agents/runs/ci-preparation/ledger-'+mode+'.json');
  const metadata={...ledger.identity(),invocation:'preparation:'+mode};
  let phase='identity-overrides',lastOwner=null;
  const commands=[];
  const diagnostic=value=>{
    const bytes=Buffer.from(value || '');
    return {text:bytes.subarray(0,64*1024).toString('utf8'),bytes:bytes.length,
      sha256:crypto.createHash('sha256').update(bytes).digest('hex'),truncated:bytes.length>64*1024};
  };
  function save(status,error) {
    const assertion=error?.message || '';
    ledger.write(receiptFile,{version:1,collection:status==='unknown' || (error?.owner && !error.owner.supervisorExited)?'partial-or-unknown':'complete',
      records:[{...metadata,project:'native-preparation',suite:'preparation',test:mode,titlePath:[mode],repeat:0,retry:0,status,
        phase,category:status==='unknown'?'unknown':error?'provisioning':'passed',assertion,signature:assertion?ledger.signature(assertion,phase):null,
        failure:error?{message:error.message,stack:error.stack,code:error.code,cause:error.cause?{message:error.cause.message,stack:error.cause.stack}:null,
          stdout:diagnostic(error.stdout || error.cause?.stdout),stderr:diagnostic(error.stderr || error.cause?.stderr),
          secondary:error.secondary || [],secondaryCap:error.secondaryCap || null}:null,
        commandOwner:error?.owner || lastOwner,commands}]});
  }
  const command=async (name,args)=>{
    phase='identity:'+path.basename(name)+':'+args.join(' ');
    if(commands.length>=32) throw Error('preparation command inventory cap exceeded');
    const shell=owned.shell();
    lastOwner=null;
    save('unknown');
    const value=await owned.run(root,shell,['-c','exec "$@"','preparation-tool',name,...args],{
      subject:path.basename(name),
      onOwner:owner=>{lastOwner=owner;save('unknown');}});
    lastOwner=value.owner;
    commands.push({phase,owner:lastOwner});
    save('unknown');
    return value.stdout.trim();
  };
  save('unknown');
  try {
  refuseOverrides();
  if(!['build','create','verify'].includes(mode)) throw Error('usage: ci-preparation.cjs build|create|verify');
  // Validate the received helper before executing any downloaded binary. The
  // later native identity/inventory check still rejects every other mismatch.
  if(mode==='verify') {
    phase='verify:supervisor-digest'; save('unknown');
    if(fs.statSync(file).size>ledger.MAX_BYTES) throw Error('preparation manifest cap exceeded');
    const manifest=JSON.parse(fs.readFileSync(file));
    const relative='target/debug/examples/fixture-owner'+(process.platform==='win32'?'.exe':'');
    if(manifest.version!==1 || manifest.identity?.os!==process.platform || manifest.identity?.arch!==process.arch
      || (process.env.GITHUB_SHA && manifest.identity.sha!==process.env.GITHUB_SHA)
      || !manifest.files?.[relative] || JSON.stringify(manifest.files[relative])!==JSON.stringify(digest(path.join(root,relative)))) throw Error('preparation supervisor source/OS/arch/digest mismatch');
  }
  if(mode==='build') {
    const boundary=await identity(root,command);
    phase='build:cargo'; save('unknown');
    const value=await owned.run(root,owned.shell(),['scripts/verify-cargo.sh',...COMMAND],{timeout:20*60*1000,maxBuffer:ledger.MAX_BYTES,
      onOwner:owner=>{lastOwner=owner;save('unknown');},onStderr:chunk=>process.stderr.write(chunk)});
    lastOwner=value.owner;commands.push({phase,owner:lastOwner});
    const output=value.stdout;
    const compiled=output.split('\n').filter(line=>line.startsWith('{')).map(line=>JSON.parse(line)).filter(value=>value.reason==='compiler-artifact' && value.executable);
    const binaries=TARGETS.map(([name,kind])=>{
      const matches=compiled.filter(value=>value.target.name===name && value.target.kind.includes(kind));
      if(matches.length!==1) throw Error('missing/ambiguous compiled preparation binary: '+name);
      const value=matches[0];
      return {name,profile:value.profile,features:value.features,executable:path.relative(root,value.executable).replace(/\\/g,'/'),digest:digest(value.executable)};
    });
    await confirmBoundary(root,boundary,command);
    phase='build:publish';save('unknown');
    ledger.write(buildFile,{version:1,identity:boundary,binaries});
  }
  else if(mode==='create') {
    const boundary=await identity(root,command),build=JSON.parse(fs.readFileSync(buildFile));
    phase='create:build-invocation';save('unknown');
    if(build.version!==1 || JSON.stringify(build.identity)!==JSON.stringify(boundary) || build.binaries?.length!==TARGETS.length || !TARGETS.every(([name],index)=>build.binaries[index].name===name)) throw Error('preparation build invocation mismatch');
    for(const binary of build.binaries) if(JSON.stringify(binary.digest)!==JSON.stringify(digest(path.join(root,binary.executable)))) throw Error('preparation compiled binary changed');
    const files=inventory(root);
    await confirmBoundary(root,boundary,command);
    phase='create:publish';save('unknown');
    ledger.write(file,{version:1,identity:boundary,compiled:build.binaries,files});
  }
  else if(mode==='verify') {
    const boundary=await identity(root,command);
    phase='verify:inventory';save('unknown');
    if(fs.statSync(file).size>ledger.MAX_BYTES) throw Error('preparation manifest cap exceeded');
    verify(JSON.parse(fs.readFileSync(file)),boundary,inventory(root));
    await confirmBoundary(root,boundary,command);
  }
  phase=mode+':complete';save('passed');
  } catch(error) {
    try { save('failed',error); }
    catch(secondary) { (error.secondary ||= []).push({message:secondary.message,stack:secondary.stack}); }
    throw error;
  }
}
if(require.main===module) main().catch(error=>{console.error(error);process.exitCode=1;});
