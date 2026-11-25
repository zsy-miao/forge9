import * as THREE from 'three';

export function makeDemo() {
  const label = document.createElement('canvas');
  label.width = 256;
  label.height = 256;
  const paint = label.getContext('2d');
  paint.fillStyle = '#27313d';
  paint.fillRect(0, 0, 256, 256);
  paint.strokeStyle = '#516678';
  paint.lineWidth = 2;
  paint.strokeRect(11, 11, 234, 234);
  for (let x = 32; x < 256; x += 32) {
    paint.beginPath();
    paint.moveTo(x, 12);
    paint.lineTo(x, 244);
    paint.moveTo(12, x);
    paint.lineTo(244, x);
    paint.stroke();
  }
  paint.fillStyle = '#7ff1d0';
  paint.fillRect(18, 216, 86, 14);
  paint.font = 'bold 39px monospace';
  paint.fillText('FG–09', 20, 67);

  const map = new THREE.CanvasTexture(label);
  map.colorSpace = THREE.SRGBColorSpace;
  const coating = new THREE.MeshStandardMaterial({
    name: 'Slate coating',
    color: '#5c638e',
    roughness: 0.55,
    metalness: 0.35,
  });
  const darkMetal = new THREE.MeshStandardMaterial({
    name: 'Dark hardware',
    color: '#2a323e',
    roughness: 0.5,
    metalness: 0.5,
  });
  const panel = new THREE.MeshStandardMaterial({ name: 'Printed panel', map, roughness: 0.82 });
  const emitter = new THREE.MeshStandardMaterial({
    name: 'Status lamp',
    color: '#68ffd1',
    emissive: '#33d7a4',
    emissiveIntensity: 0.45,
    roughness: 0.45,
  });

  const root = new THREE.Group();
  function part(name, geometry, material, position) {
    const mesh = new THREE.Mesh(geometry, material);
    mesh.name = name;
    mesh.position.set(...position);
    root.add(mesh);
    return mesh;
  }

  const body = part(
    'Signal housing', new THREE.CylinderGeometry(0.96, 1.05, 1.15, 8), coating, [0, 0.66, 0],
  );
  body.rotation.y = Math.PI / 8;
  part('Mounting foot', new THREE.BoxGeometry(1.85, 0.17, 1.85), darkMetal, [0, 0.09, 0]);
  part('Service panel', new THREE.BoxGeometry(0.73, 0.78, 0.1), panel, [0, 0.67, 0.94]);
  part('Upper collar', new THREE.CylinderGeometry(0.31, 0.41, 0.19, 12), darkMetal, [0, 1.32, 0]);
  part('Emitter', new THREE.CylinderGeometry(0.24, 0.28, 0.15, 12), emitter, [0, 1.49, 0]);
  part('Locator strip', new THREE.BoxGeometry(0.58, 0.04, 0.04), emitter, [0, 0.28, 1.03]);
  return root;
}
