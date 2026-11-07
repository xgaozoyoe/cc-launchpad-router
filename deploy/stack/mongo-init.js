const root = new Mongo('mongodb://'+encodeURIComponent(process.env.MONGO_INITDB_ROOT_USERNAME)+':'+encodeURIComponent(process.env.MONGO_INITDB_ROOT_PASSWORD)+'@mongo:27017/admin?directConnection=true');
const admin=root.getDB('admin');
let status=admin.runCommand({replSetGetStatus:1});
if(status.code===94){assert.commandWorked(admin.runCommand({replSetInitiate:{_id:'rs0',members:[{_id:0,host:'mongo:27017'}]}}));}
for(let i=0;i<120;i++){if(admin.hello().isWritablePrimary)break;if(i===119)throw new Error('Replica set did not become primary');sleep(1000);}
const app=root.getDB('launchpad');
if(!app.getUser('launchpad'))app.createUser({user:'launchpad',pwd:process.env.MONGO_APP_PASSWORD,roles:[{role:'readWrite',db:'launchpad'}]});
