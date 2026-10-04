// Независимый CPU oracle: quaternion/translation + orthographic frustum.
// Production view-projection matrices/Three.js.project здесь не используются.
import assert from "node:assert/strict";

export function rotateByQuaternion(vector, quaternion) {
  const [x,y,z,w] = quaternion;
  const [vx,vy,vz] = vector;
  const tx = 2*(y*vz-z*vy), ty = 2*(z*vx-x*vz), tz = 2*(x*vy-y*vx);
  return [vx+w*tx+y*tz-z*ty, vy+w*ty+z*tx-x*tz, vz+w*tz+x*ty-y*tx];
}

export function orthographicOracle(positionMeters, audit) {
  const { camera:c, sceneScale:scale, viewport:v } = audit;
  assert.ok(positionMeters.length===3 && positionMeters.every(Number.isFinite));
  assert.ok(Number.isFinite(scale) && scale>0);
  assert.ok(Math.abs(Math.hypot(...c.quaternion)-1)<1e-12, "Unit camera quaternion");
  assert.ok(c.right>c.left && c.top>c.bottom && c.far>c.near && c.zoom>0);
  const translated = positionMeters.map((value,axis)=>value*scale-c.position[axis]);
  const q=c.quaternion;
  const local=rotateByQuaternion(translated,[-q[0],-q[1],-q[2],q[3]]);
  const centerX=(c.right+c.left)/2, centerY=(c.top+c.bottom)/2;
  const nx=2*c.zoom*(local[0]-centerX)/(c.right-c.left);
  const ny=2*c.zoom*(local[1]-centerY)/(c.top-c.bottom);
  const nz=-(2*local[2]+c.far+c.near)/(c.far-c.near);
  return { x:v.left+(nx+1)*v.width/2, y:v.top+(1-ny)*v.height/2,
    ndc:[nx,ny,nz], local, inView:[nx,ny,nz].every(value=>value>=-1&&value<=1) };
}

export function exactCandidateOracle(markers, x, y) {
  return markers.map(marker=>({...marker,distance:Math.hypot(marker.x-x,marker.y-y)}))
    .filter(marker=>marker.distance<=marker.r)
    .sort((a,b)=>a.distance-b.distance||(BigInt(a.id)<BigInt(b.id)?-1:BigInt(a.id)>BigInt(b.id)?1:0));
}

export function selfCheck() {
  const a={sceneScale:1, viewport:{left:10,top:20,width:400,height:200},
    camera:{position:[0,0,5],quaternion:[0,0,0,1],left:-2,right:2,top:1,bottom:-1,near:1,far:9,zoom:1}};
  let p=orthographicOracle([0,0,0],a);assert.deepEqual([p.x,p.y],[210,120]);assert.ok(Math.abs(p.ndc[2])===0);
  p=orthographicOracle([1,0,0],a);assert.equal(p.x,310);
  p=orthographicOracle([0,1,0],a);assert.equal(p.y,20);
  p=orthographicOracle([0,0,4],a);assert.equal(p.ndc[2],-1);
  p=orthographicOracle([0,0,-4],a);assert.equal(p.ndc[2],1);
  const angle=Math.PI/2, q=[0,Math.sin(angle/2),0,Math.cos(angle/2)];
  const v=rotateByQuaternion([0,0,1],q);assert.ok(Math.abs(v[0]-1)<1e-14&&Math.abs(v[2])<1e-14);
  const inverse=rotateByQuaternion(v,[-q[0],-q[1],-q[2],q[3]]);
  assert.ok(Math.abs(inverse[0])<1e-14&&Math.abs(inverse[2]-1)<1e-14);
  const offcenter={...a,camera:{...a.camera,left:-1,right:3,top:2,bottom:-2,zoom:2}};
  p=orthographicOracle([1,0,0],offcenter);assert.deepEqual([p.x,p.y],[210,120]);
  const nonsquare={...a,sceneScale:1/200e-6};
  const o=orthographicOracle([0,0,0],nonsquare);
  const ux=orthographicOracle([20e-6,0,0],nonsquare), uy=orthographicOracle([0,20e-6,0],nonsquare);
  assert.ok(Math.abs((ux.x-o.x)-(o.y-uy.y))<1e-12);
  const rotated={...a,camera:{...a.camera,position:[5,0,0],quaternion:q}};
  const before=orthographicOracle([0,0,0],rotated), after=orthographicOracle([0,0,1],rotated);
  assert.ok(Math.abs((after.x-before.x)+100)<1e-12&&Math.abs(after.y-before.y)<1e-12);
  const ids=['18446744073709551615','9007199254740993','9007199254740992'];
  assert.deepEqual(exactCandidateOracle(ids.map(id=>({id,x:3,y:4,r:5})),0,0).map(m=>m.id),ids.toReversed());
  return {status:'PASS',scope:'Synthetic independent oracle self-check only; no production renderer or browser acceptance.'};
}
