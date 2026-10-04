// The existing lane's preparation, reused only at the same native build boundary.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const {execFileSync}=require('node:child_process');
const ledger=require('./ci-ledger.cjs');
const MANIFEST='agents/runs/ci-preparation/manifest.json';
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
  const command=(name,args)=>execFileSync(name,args,{cwd:root,encoding:'utf8',timeout:5000,maxBuffer:64*1024}).trim();
  const sha=command('git',['rev-parse','HEAD']);
  command('git',['diff','--quiet','HEAD','--']);
  if(process.env.GITHUB_SHA && process.env.GITHUB_SHA!==sha) throw Error('preparation checkout differs from tested SHA');
  return {sha,os:process.platform,arch:process.arch,profile:'dev',features:'default',target:'host',
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
  if(mode==='create') ledger.write(file,{version:1,identity:identity(root),files:inventory(root)});
  else if(mode==='verify') {
    if(fs.statSync(file).size>ledger.MAX_BYTES) throw Error('preparation manifest cap exceeded');
    verify(JSON.parse(fs.readFileSync(file)),identity(root),inventory(root));
  } else throw Error('usage: ci-preparation.cjs create|verify');
}
