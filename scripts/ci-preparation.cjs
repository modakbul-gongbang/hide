// The existing lane's preparation, reused only at the same native build boundary.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const {execFileSync}=require('node:child_process');
const ledger=require('./ci-ledger.cjs');
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
function identity(root) {
  // This lane has exactly one supported native build recipe. Cargo accepts
  // profile/target overrides from the environment even when argv is unchanged.
  // Refuse those before both build and reuse instead of labelling them default.
  const overrides=Object.keys(process.env).filter(key=>key.startsWith('CARGO_PROFILE_') || key==='CARGO_BUILD_TARGET' || key==='CARGO_BUILD_RUSTC' || key==='CI_PREPARATION_FEATURES');
  if(overrides.length) throw Error('unsupported preparation build override: '+overrides.sort().join(', '));
  // Verification already owns a Bash entrypoint on every runner. Its command
  // lookup executes Windows pnpm command shims as well as native executables;
  // Node's execFile lookup alone cannot start that same installed shim.
  // Pass separate argv through a fixed script, never interpolate shell text.
  const command=(name,args)=>execFileSync('bash',['-c','"$@"','preparation-tool',name,...args],{cwd:root,encoding:'utf8',timeout:5000,maxBuffer:64*1024}).trim();
  const sha=command('git',['rev-parse','HEAD']);
  command('git',['diff','--quiet','HEAD','--']);
  if(process.env.GITHUB_SHA && process.env.GITHUB_SHA!==sha) throw Error('preparation checkout differs from tested SHA');
  return {sha,os:process.platform,arch:process.arch,command:COMMAND,
    rust:command('rustc',['-vV']),node:process.version,pnpm:command('pnpm',['--version']),
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
function verify(value, expected, files) {
  if(value.version!==1 || JSON.stringify(value.identity)!==JSON.stringify(expected)) throw Error('preparation source/profile/features/OS/arch/toolchain mismatch');
  if(JSON.stringify(value.files)!==JSON.stringify(files)) throw Error('preparation output inventory or digest mismatch');
}
module.exports={identity,inventory,verify,digest};
if(require.main===module) {
  const root=process.cwd(), mode=process.argv[2], file=path.join(root,MANIFEST);
  const buildFile=path.join(root,BUILD);
  if(mode==='build') {
    const boundary=identity(root);
    const output=execFileSync('bash',['scripts/verify-cargo.sh',...COMMAND],{cwd:root,encoding:'utf8',timeout:20*60*1000,maxBuffer:ledger.MAX_BYTES,stdio:['ignore','pipe','inherit']});
    const compiled=output.split('\n').filter(line=>line.startsWith('{')).map(line=>JSON.parse(line)).filter(value=>value.reason==='compiler-artifact' && value.executable);
    const binaries=TARGETS.map(([name,kind])=>{
      const matches=compiled.filter(value=>value.target.name===name && value.target.kind.includes(kind));
      if(matches.length!==1) throw Error('missing/ambiguous compiled preparation binary: '+name);
      const value=matches[0];
      return {name,profile:value.profile,features:value.features,executable:path.relative(root,value.executable).replace(/\\/g,'/'),digest:digest(value.executable)};
    });
    ledger.write(buildFile,{version:1,identity:boundary,binaries});
  }
  else if(mode==='create') {
    const boundary=identity(root),build=JSON.parse(fs.readFileSync(buildFile));
    if(build.version!==1 || JSON.stringify(build.identity)!==JSON.stringify(boundary) || build.binaries?.length!==TARGETS.length || !TARGETS.every(([name],index)=>build.binaries[index].name===name)) throw Error('preparation build invocation mismatch');
    for(const binary of build.binaries) if(JSON.stringify(binary.digest)!==JSON.stringify(digest(path.join(root,binary.executable)))) throw Error('preparation compiled binary changed');
    ledger.write(file,{version:1,identity:boundary,compiled:build.binaries,files:inventory(root)});
  }
  else if(mode==='verify') {
    const boundary=identity(root);
    if(fs.statSync(file).size>ledger.MAX_BYTES) throw Error('preparation manifest cap exceeded');
    verify(JSON.parse(fs.readFileSync(file)),boundary,inventory(root));
  } else throw Error('usage: ci-preparation.cjs build|create|verify');
}
