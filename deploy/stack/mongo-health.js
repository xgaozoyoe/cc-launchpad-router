const c=new Mongo('mongodb://'+encodeURIComponent(process.env.MONGO_INITDB_ROOT_USERNAME)+':'+encodeURIComponent(process.env.MONGO_INITDB_ROOT_PASSWORD)+'@127.0.0.1:27017/admin?directConnection=true');
const r=c.getDB('admin').runCommand({getCmdLineOpts:1});
quit(r.ok && r.parsed?.replication?.replSetName==='rs0' ? 0 : 1);
