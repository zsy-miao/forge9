import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { makeDemo } from './demo.js';
import './style.css';

const el = (id) => document.getElementById(id);
const viewport = el('viewport');
const previewImage = el('preview-image');
const picker = el('file-input');
const dropZone = el('drop-zone');
const wireframe = el('wireframe');
const showGrid = el('grid');
const resetView = el('reset-view');
const canvasScene = new THREE.Scene();
const numbers = new Intl.NumberFormat();
const axisLength = new Intl.NumberFormat(undefined, { maximumSignificantDigits: 3 });
const axes = ['x', 'y', 'z'];

let stage;
let asset;
let imageUrl;
let request = 0;

function setStatus(message, error = false) {
  const status = el('status');
  status.textContent = message;
  status.classList.toggle('is-error', error);
}

function render() {
  if (stage && !viewport.hidden && viewport.clientWidth > 0 && viewport.clientHeight > 0) {
    stage.renderer.render(canvasScene, stage.camera);
  }
}

function resize() {
  const width = viewport.clientWidth;
  const height = viewport.clientHeight;
  if (!stage || !width || !height) return;
  stage.camera.aspect = width / height;
  stage.camera.updateProjectionMatrix();
  stage.renderer.setSize(width, height, false);
  render();
}

function ensureStage() {
  if (stage) return;
  let renderer;
  try {
    renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  } catch {
    throw new Error('3D previews need WebGL 2. You can still open a texture image.');
  }
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.domElement.tabIndex = 0;
  renderer.domElement.setAttribute('aria-label', 'Interactive model preview');
  viewport.append(renderer.domElement);

  const camera = new THREE.PerspectiveCamera(37, 1, 0.01, 5000);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.listenToKeyEvents(renderer.domElement);
  controls.addEventListener('change', render);

  const key = new THREE.DirectionalLight('#fff0dd', 3.1);
  key.position.set(5, 9, 7);
  const fill = new THREE.DirectionalLight('#a0b7ff', 1.4);
  fill.position.set(-4, 4, -5);
  const grid = new THREE.GridHelper(16, 16, '#637395', '#3c4963');
  grid.material.transparent = true;
  grid.material.opacity = 0.55;
  grid.visible = showGrid.checked;
  canvasScene.add(new THREE.HemisphereLight('#dfedff', '#434758', 2.6), key, fill, grid);
  stage = { renderer, camera, controls, grid };

  new ResizeObserver(resize).observe(viewport);
  renderer.domElement.addEventListener('webglcontextlost', () => {
    setStatus('The graphics context was interrupted. Reload the viewer to restore 3D previews.', true);
  });
}

function inspectModel(...roots) {
  const meshes = [];
  const geometries = new Set();
  const materials = new Set();
  const textures = new Set();
  let vertices = 0;
  let triangles = 0;
  const collectMesh = (object) => {
    if (!object.isMesh) return;
    const { geometry } = object;
    const vertexCount = geometry.getAttribute('position')?.count ?? 0;
    meshes.push(object);
    geometries.add(geometry);
    vertices += vertexCount;
    triangles += Math.floor((geometry.index?.count ?? vertexCount) / 3);
    for (const material of [].concat(object.material).filter(Boolean)) {
      if (materials.has(material)) continue;
      materials.add(material);
      for (const value of Object.values(material)) {
        if (value?.isTexture) textures.add(value);
      }
    }
  };
  for (const root of roots) root.traverse(collectMesh);
  return { meshes, geometries, materials, textures, vertices, triangles };
}

function disposeScenes(scenes) {
  const model = inspectModel(...scenes);
  for (const geometry of model.geometries) geometry.dispose();
  for (const material of model.materials) material.dispose();
  const images = new Set();
  for (const texture of model.textures) {
    texture.dispose();
    if (typeof texture.image?.close === 'function') images.add(texture.image);
  }
  for (const image of images) image.close();
}

function clearAsset() {
  if (asset) {
    canvasScene.remove(asset.root);
    disposeScenes(asset.scenes);
    asset = undefined;
  }
  if (imageUrl) {
    previewImage.removeAttribute('src');
    previewImage.alt = '';
    URL.revokeObjectURL(imageUrl);
    imageUrl = undefined;
  }
  el('mesh-list').replaceChildren();
}

function setInspector(name, kind, dimensions, model) {
  el('object-name').textContent = name;
  el('asset-kind').textContent = kind;
  el('dimensions').textContent = dimensions;
  for (const [id, count] of Object.entries({
    'mesh-count': model?.meshes.length,
    'material-count': model?.materials.size,
    'texture-count': model?.textures.size ?? 1,
    'vertex-count': model?.vertices,
    'triangle-count': model?.triangles,
  })) {
    el(id).textContent = count === undefined ? '—' : numbers.format(count);
  }

  const list = document.createDocumentFragment();
  model?.meshes.forEach((mesh, index) => {
    const row = document.createElement('label');
    row.className = 'mesh-row';
    const toggle = document.createElement('input');
    toggle.type = 'checkbox';
    toggle.checked = mesh.visible;
    toggle.addEventListener('change', () => {
      mesh.visible = toggle.checked;
      render();
    });
    const meshName = document.createElement('span');
    meshName.className = 'mesh-name';
    meshName.textContent = mesh.name || `Mesh ${index + 1}`;
    const description = document.createElement('small');
    description.className = 'mesh-detail';
    description.textContent = [].concat(mesh.material)
      .map((material) => material?.name || material?.type)
      .filter(Boolean)
      .join(' · ');
    row.append(toggle, meshName, description);
    list.append(row);
  });
  el('mesh-list').replaceChildren(list);
}

function fitView() {
  if (!asset || !stage) return;
  const { camera, controls, grid } = stage;
  const sphere = asset.bounds.getBoundingSphere(new THREE.Sphere());
  const radius = Math.max(sphere.radius, 0.001);
  const vertical = THREE.MathUtils.degToRad(camera.fov / 2);
  const angle = Math.min(vertical, Math.atan(Math.tan(vertical) * camera.aspect));
  const distance = 1.16 * radius / Math.sin(angle);
  controls.target.copy(sphere.center);
  camera.position.copy(sphere.center)
    .addScaledVector(new THREE.Vector3(1, 0.65, 1.35).normalize(), distance);
  camera.near = radius / 1000;
  camera.far = distance * 12;
  camera.updateProjectionMatrix();
  controls.minDistance = radius * 0.1;
  controls.maxDistance = distance * 7;
  grid.position.set(sphere.center.x, asset.bounds.min.y, sphere.center.z);
  grid.scale.setScalar(radius / 4);
  controls.update();
  render();
}

function syncWireframe() {
  if (!asset) return;
  for (const [material, original] of asset.wireframes) {
    material.wireframe = wireframe.checked || original;
  }
  render();
}

function showModel(root, name, kind, scenes = [root]) {
  const model = inspectModel(root);
  try {
    ensureStage();
    const bounds = new THREE.Box3().setFromObject(root);
    const size = bounds.getSize(new THREE.Vector3());
    if (bounds.isEmpty() || !model.vertices || axes.some((axis) => !Number.isFinite(size[axis]))) {
      throw new Error('This file has no renderable mesh.');
    }
    clearAsset();
    asset = {
      ...model,
      root,
      scenes,
      bounds,
      wireframes: new Map(Array.from(model.materials)
        .filter((material) => 'wireframe' in material)
        .map((material) => [material, material.wireframe])),
    };
    canvasScene.add(root);
    viewport.hidden = false;
    previewImage.hidden = true;
    resetView.disabled = false;
    wireframe.disabled = false;
    showGrid.disabled = false;
    setInspector(name, kind, axes.map((axis) => axisLength.format(size[axis])).join(' × '), model);
    resize();
    fitView();
    syncWireframe();
  } catch (error) {
    if (asset?.root === root) clearAsset();
    else disposeScenes(scenes);
    throw error;
  }
}

function validateGlb(buffer) {
  const header = new DataView(buffer);
  if (buffer.byteLength < 20
    || header.getUint32(0, true) !== 0x46546c67
    || header.getUint32(4, true) !== 2
    || header.getUint32(8, true) !== buffer.byteLength
    || header.getUint32(16, true) !== 0x4e4f534a) {
    throw new Error('Choose a valid glTF 2.0 binary (.glb).');
  }
  const length = header.getUint32(12, true);
  if (length > buffer.byteLength - 20) throw new Error('The GLB is truncated.');
  const info = JSON.parse(new TextDecoder().decode(new Uint8Array(buffer, 20, length)));
  // Reject dependencies before any texture or buffer loader runs, including stale blob URLs.
  for (const source of [...(info.buffers ?? []), ...(info.images ?? [])]) {
    if (source.uri !== undefined && !/^data:/i.test(source.uri)) {
      throw new Error('This GLB references external files. Export it with embedded buffers and textures.');
    }
  }
}

const manager = new THREE.LoadingManager();
manager.setURLModifier((url) => {
  if (/^(blob|data):/i.test(url)) return url;
  throw new Error('External GLB dependencies are unavailable. Export a self-contained GLB.');
});
const loader = new GLTFLoader(manager);

async function openFile(file) {
  if (!file) return;
  const load = ++request;
  setStatus(`Opening ${file.name}…`);
  try {
    if (/\.glb$/i.test(file.name)) {
      const bytes = await file.arrayBuffer();
      validateGlb(bytes);
      const model = await loader.parseAsync(bytes, '');
      if (load !== request) return disposeScenes(model.scenes);
      showModel(model.scene, file.name, 'glTF 2.0 binary', model.scenes);
      setStatus('Model ready.');
    } else if (/\.(png|jpe?g|webp)$/i.test(file.name)) {
      const url = URL.createObjectURL(file);
      const decoded = new Image();
      decoded.src = url;
      try {
        await decoded.decode();
        if (load !== request) return;
        clearAsset();
        imageUrl = url;
        previewImage.src = url;
        previewImage.alt = file.name;
        previewImage.hidden = false;
        viewport.hidden = true;
        resetView.disabled = true;
        wireframe.disabled = true;
        showGrid.disabled = true;
        setInspector(file.name, 'Texture image', `${decoded.naturalWidth} × ${decoded.naturalHeight} px`);
        setStatus('Texture ready.');
      } finally {
        if (imageUrl !== url) URL.revokeObjectURL(url);
      }
    } else {
      throw new Error('Choose a GLB model or a PNG, JPEG, or WebP image.');
    }
  } catch (error) {
    if (load === request) setStatus(error.message || 'Unable to open this file.', true);
  }
}

function showDemo() {
  request++;
  try {
    showModel(makeDemo(), 'Signal beacon', 'Synthetic sample');
    setStatus('Sample model ready.');
  } catch (error) {
    setStatus(error.message, true);
  }
}

picker.addEventListener('change', () => {
  openFile(picker.files[0]);
  picker.value = '';
});
dropZone.addEventListener('dragover', (event) => {
  event.preventDefault();
  dropZone.classList.add('drag-active');
});
dropZone.addEventListener('dragleave', (event) => {
  if (!dropZone.contains(event.relatedTarget)) dropZone.classList.remove('drag-active');
});
dropZone.addEventListener('drop', (event) => {
  event.preventDefault();
  dropZone.classList.remove('drag-active');
  openFile(event.dataTransfer.files[0]);
});
resetView.addEventListener('click', fitView);
wireframe.addEventListener('change', syncWireframe);
showGrid.addEventListener('change', () => {
  if (stage) stage.grid.visible = showGrid.checked;
  render();
});
el('demo').addEventListener('click', showDemo);
showDemo();
