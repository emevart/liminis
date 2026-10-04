// Дополнительный renderer центров. SI snapshot остаётся у общего viewer;
// этот модуль хранит только текущие display objects и camera, не модель.
export const CENTER_PIXELS = 7;
export const HIT_PIXELS = 8;

export function physicalScale(dimensions) {
  if (!Array.isArray(dimensions) || dimensions.length !== 3 || !dimensions.every(value => typeof value === "number" && Number.isFinite(value) && value > 0 && Number.isFinite(value * 1e6))) throw new Error("Invalid physical dimensions");
  const scale = 1 / Math.max(...dimensions);
  if (!Number.isFinite(scale) || scale <= 0) throw new Error("Unsupported physical display scale");
  return scale;
}

export function validateCenter(cell, dimensions) {
  if (typeof cell.id !== "string" || !/^\d+$/.test(cell.id)) throw new Error("Invalid exact cell ID");
  const position = cell.position_m;
  if (!Array.isArray(position) || position.length !== 3 || !position.every((value, axis) => typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= dimensions[axis])) throw new Error("Invalid physical position");
}

export function physicalSlice(dimensions, axis, slice) {
  if (![0, 1, 2].includes(axis)) throw new Error("Invalid slice axis");
  const length = dimensions[axis];
  if (!slice.enabled) return [0, length];
  if (!Number.isFinite(slice.center) || slice.center < 0 || slice.center > length || !Number.isFinite(slice.thickness) || slice.thickness < 0) throw new Error("Invalid physical slice");
  return [Math.max(0, slice.center - slice.thickness / 2), Math.min(length, slice.center + slice.thickness / 2)];
}

// Column-major view-projection matrix камеры; координаты остаются binary64
// для hit map. GPU округление относится только к рисунку, не inspector.
export function projectCenter(position, scale, matrix, viewport) {
  const [x, y, z] = position.map(value => value * scale), m = matrix;
  const w = m[3] * x + m[7] * y + m[11] * z + m[15];
  const ndc = [(m[0] * x + m[4] * y + m[8] * z + m[12]) / w, (m[1] * x + m[5] * y + m[9] * z + m[13]) / w, (m[2] * x + m[6] * y + m[10] * z + m[14]) / w];
  if (!Number.isFinite(w) || w <= 0 || !ndc.every(Number.isFinite)) throw new Error("Unsupported camera projection");
  return { x: viewport.left + (ndc[0] + 1) * viewport.width / 2, y: viewport.top + (1 - ndc[1]) * viewport.height / 2, inView: ndc.every(value => value >= -1 && value <= 1) };
}

export function visibilityOf(position, axis, interval, layers, projected) {
  if (position[axis] < interval[0] || position[axis] > interval[1]) return "outside slice";
  if (!layers.centers || layers.opacity === 0) return "centers layer hidden";
  return projected.inView ? "visible" : "outside current view";
}

export function tapGesture({ distance = 4, duration = 450 } = {}) {
  let gesture = null;
  return {
    down(event, now, revision) {
      if (gesture) { gesture.cancelled = true; return; }
      if (event.button !== 0 || event.isPrimary === false) return;
      gesture = { id: event.pointerId, x: event.clientX, y: event.clientY, now, revision, moved: 0, cancelled: false };
    },
    move(event) { if (gesture?.id === event.pointerId) gesture.moved = Math.max(gesture.moved, Math.hypot(event.clientX - gesture.x, event.clientY - gesture.y)); },
    changed() { if (gesture) gesture.cancelled = true; },
    cancel() { gesture = null; },
    up(event, now, revision) {
      if (gesture?.id !== event.pointerId) return false;
      this.move(event); const accepted = !gesture.cancelled && gesture.revision === revision && gesture.moved <= distance && now - gesture.now >= 0 && now - gesture.now <= duration;
      gesture = null; return accepted;
    },
  };
}

function circleTexture(THREE, ring = false) {
  const size = 32, data = new Uint8Array(size * size * 4);
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
    const radius = Math.hypot(x + .5 - size / 2, y + .5 - size / 2) / (size / 2), index = (y * size + x) * 4;
    data[index] = data[index + 1] = data[index + 2] = 255;
    data[index + 3] = Math.round(255 * Math.max(0, Math.min(1, (1 - radius) * size / 2)) * (ring ? Math.max(0, Math.min(1, (radius - .7) * size / 2)) : 1));
  }
  const texture = new THREE.DataTexture(data, size, size, THREE.RGBAFormat);
  texture.magFilter = texture.minFilter = THREE.LinearFilter; texture.generateMipmaps = false; texture.needsUpdate = true; return texture;
}

export function createPhysical3D({ THREE, OrbitControls, canvas, palette, colorIndex, onChange, onLost, rendererFactory }) {
  // Единственная точка тестовой инъекции: allocation/lifecycle WebGL. Геометрия
  // и объекты камеры/сцены в unit tests остаются настоящими Three.js.
  const renderer = rendererFactory ? rendererFactory(canvas) : new THREE.WebGLRenderer({ canvas, antialias: true });
  const allocated = [renderer];
  function own(resource) { allocated.push(resource); return resource; }
  function release() { for (const resource of allocated.reverse()) resource.dispose(); allocated.length = 0; }
  try {
    renderer.setClearColor("#080d0b");
    const scene = new THREE.Scene(), camera = new THREE.OrthographicCamera(-1, 1, 1, -1, .01, 100);
    const controls = own(new OrbitControls(camera, canvas)); controls.enableDamping = false; controls.autoRotate = false; controls.minZoom = .05; controls.maxZoom = 1000;
    // listenToKeyEvents намеренно не вызывается: стрелки выбирают точные IDs.
    const texture = own(circleTexture(THREE)), ringTexture = own(circleTexture(THREE, true));
    const materials = palette.map(color => own(new THREE.SpriteMaterial({ map: texture, color, transparent: true, depthWrite: false, depthTest: false, toneMapped: false })));
    const selectionMaterial = own(new THREE.SpriteMaterial({ map: ringTexture, color: "#ffffff", transparent: true, depthWrite: false, depthTest: false, toneMapped: false }));
    const selection = new THREE.Sprite(selectionMaterial); selection.renderOrder = 11; selection.visible = false;
    own(selection.geometry);
    const centers = new THREE.Group(), box = new THREE.Group(), boundaries = new THREE.Group(); scene.add(box, boundaries, centers, selection);
    const sprites = new Map(), visibility = new Map(), geometries = [], lineMaterials = [];
    function line(parent, color, loop = false) {
      const geometry = own(new THREE.BufferGeometry()), material = own(new THREE.LineBasicMaterial({ color, transparent: true, opacity: .8, depthTest: false, depthWrite: false, toneMapped: false }));
      const object = loop ? new THREE.LineLoop(geometry, material) : new THREE.LineSegments(geometry, material);
      object.frustumCulled = false; parent.add(object); geometries.push(geometry); lineMaterials.push(material); return object;
    }
    const edges = line(box, "#77978a"), axes = ["#ff8f83", "#86dfb7", "#72d5df"].map(color => line(box, color)), planes = [line(boundaries, "#dfbd64", true), line(boundaries, "#dfbd64", true)];
    let disposed = false, lost = false, scale = null, dimensions = null, viewport = null, hits = [], visibleSprites = 0;
    function vertices(object, values) {
      const attribute = object.geometry.getAttribute("position");
      if (attribute?.array.length === values.length) { attribute.array.set(values); attribute.needsUpdate = true; }
      else object.geometry.setAttribute("position", new THREE.Float32BufferAttribute(values, 3));
      object.geometry.computeBoundingSphere();
    }
    function resize(value) {
      if (![value.left, value.top, value.width, value.height, value.canvasWidth, value.canvasHeight].every(Number.isFinite) || value.width <= 0 || value.height <= 0 || value.canvasWidth <= 0 || value.canvasHeight <= 0) throw new Error("Invalid 3D viewport");
      const changed = !viewport || ["left", "top", "width", "height", "canvasWidth", "canvasHeight", "pixelRatio"].some(key => viewport[key] !== value[key]);
      viewport = { ...value };
      if (changed) {
        renderer.setPixelRatio(Math.min(2, Math.max(1, value.pixelRatio || 1))); renderer.setSize(value.canvasWidth, value.canvasHeight, false);
        renderer.setViewport(value.left, value.canvasHeight - value.top - value.height, value.width, value.height);
        if (dimensions) fitFrustum(); onChange();
      }
    }
    function fitFrustum() {
      const radius = Math.hypot(...dimensions.map(value => value * scale)) / 2, aspect = viewport.width / viewport.height;
      const half = radius * 1.25 * Math.max(1, 1 / aspect);
      camera.left = -half * aspect; camera.right = half * aspect; camera.top = half; camera.bottom = -half; camera.updateProjectionMatrix();
    }
    function reset() {
      if (disposed || !dimensions || !viewport) return;
      const center = new THREE.Vector3(...dimensions.map(value => value * scale / 2)), radius = Math.hypot(...dimensions.map(value => value * scale)) / 2;
      fitFrustum(); camera.up.set(0, 1, 0); camera.position.copy(center).add(new THREE.Vector3(1, 1, 1).normalize().multiplyScalar(radius * 4 + 1));
      controls.target.copy(center); camera.zoom = 1; camera.lookAt(center); camera.updateProjectionMatrix(); controls.update(); controls.saveState(); onChange();
    }
    function setBox(next) {
      const changed = !dimensions || next.some((value, axis) => value !== dimensions[axis]);
      scale = physicalScale(next); dimensions = [...next];
      if (!changed) return;
      const lengths = next.map(value => value * scale), corners = Array.from({ length: 8 }, (_, index) => lengths.map((value, axis) => (index >> axis & 1) ? value : 0)), pairs = [];
      for (let index = 0; index < 8; index++) for (let axis = 0; axis < 3; axis++) if (!(index >> axis & 1)) pairs.push(...corners[index], ...corners[index | 1 << axis]);
      vertices(edges, pairs); axes.forEach((object, axis) => { const end = [0, 0, 0]; end[axis] = lengths[axis]; vertices(object, [0, 0, 0, ...end]); }); reset();
    }
    function clear() {
      for (const sprite of sprites.values()) centers.remove(sprite); sprites.clear(); visibility.clear(); hits = []; visibleSprites = 0; selection.visible = false; controls.enabled = false;
    }
    function draw(state, options, nextViewport) {
      if (disposed || lost) throw new Error("3D renderer is unavailable");
      const nextDimensions = state.model?.dimensions_m; physicalScale(nextDimensions);
      const ids = new Set(); for (const cell of state.cells) { validateCenter(cell, nextDimensions); if (ids.has(cell.id)) throw new Error("Duplicate exact cell ID"); ids.add(cell.id); }
      resize(nextViewport); setBox(nextDimensions); controls.enabled = true;
      const interval = physicalSlice(dimensions, options.axis, options.slice), others = [0, 1, 2].filter(axis => axis !== options.axis);
      if (typeof options.layers.opacity !== "number" || !Number.isFinite(options.layers.opacity) || options.layers.opacity < 0 || options.layers.opacity > 1) throw new Error("Invalid centers opacity");
      box.visible = options.layers.box; boundaries.visible = options.layers.slice && options.slice.enabled;
      planes.forEach((object, index) => { const values = []; for (const [u, v] of [[0, 0], [1, 0], [1, 1], [0, 1]]) { const point = [0, 0, 0]; point[options.axis] = interval[index] * scale; point[others[0]] = u * dimensions[others[0]] * scale; point[others[1]] = v * dimensions[others[1]] * scale; values.push(...point); } vertices(object, values); object.visible = index === 0 || interval[0] !== interval[1]; });
      for (const [id, sprite] of sprites) if (!ids.has(id)) { centers.remove(sprite); sprites.delete(id); }
      materials.forEach(material => material.opacity = options.layers.opacity);
      camera.updateMatrixWorld(); const matrix = new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse).elements;
      const diameter = CENTER_PIXELS * (camera.top - camera.bottom) / camera.zoom / viewport.height;
      hits = []; visibility.clear(); visibleSprites = 0; selection.visible = false;
      for (const cell of state.cells) {
        let sprite = sprites.get(cell.id); if (!sprite) { sprite = new THREE.Sprite(materials[colorIndex(cell.genome_key)]); sprite.renderOrder = 10; sprite.frustumCulled = false; sprites.set(cell.id, sprite); centers.add(sprite); }
        sprite.material = materials[colorIndex(cell.genome_key)]; sprite.position.set(...cell.position_m.map(value => value * scale)); sprite.scale.set(diameter, diameter, 1);
        const projected = projectCenter(cell.position_m, scale, matrix, viewport), status = visibilityOf(cell.position_m, options.axis, interval, options.layers, projected); visibility.set(cell.id, status); sprite.visible = status === "visible";
        if (sprite.visible) { visibleSprites++; hits.push({ id: cell.id, genome_key: cell.genome_key, x: projected.x, y: projected.y, r: HIT_PIXELS }); }
        if (sprite.visible && cell.id === options.selected) { selection.position.copy(sprite.position); selection.scale.set(diameter * 13 / CENTER_PIXELS, diameter * 13 / CENTER_PIXELS, 1); selection.visible = true; }
      }
      renderer.clear(); renderer.render(scene, camera); return hits;
    }
    const changed = () => { if (!disposed) onChange(); }, contextLost = event => { event.preventDefault(); if (disposed || lost) return; lost = true; controls.enabled = false; onLost(new Error("WebGL context lost")); };
    controls.addEventListener("change", changed); canvas.addEventListener("webglcontextlost", contextLost);
    let backend = { vendor: null, renderer: null, version: null };
    try { const gl = renderer.getContext(), extension = gl.getExtension("WEBGL_debug_renderer_info"); backend = { vendor: extension ? gl.getParameter(extension.UNMASKED_VENDOR_WEBGL) : gl.getParameter(gl.VENDOR), renderer: extension ? gl.getParameter(extension.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER), version: gl.getParameter(gl.VERSION) }; } catch {}
    return {
      draw, reset, clear, visibility: id => visibility.get(id) ?? "not present",
      axisLabels() {
        camera.updateMatrixWorld();const matrix=new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse).elements;
        return dimensions.map((length,axis)=>{const point=[0,0,0];point[axis]=length;return projectCenter(point,scale,matrix,viewport);});
      },
      audit() {
        const rect = canvas.getBoundingClientRect();
        return { dimensions_m: dimensions ? [...dimensions] : null, sceneScale: scale,
          camera: { position: camera.position.toArray(), target: controls.target.toArray(), up: camera.up.toArray(), quaternion: camera.quaternion.toArray(), left: camera.left, right: camera.right, top: camera.top, bottom: camera.bottom, near: camera.near, far: camera.far, zoom: camera.zoom },
          viewport: viewport ? { ...viewport, pixelRatio: renderer.getPixelRatio(), rect: { left: rect.left, top: rect.top, width: rect.width, height: rect.height } } : null,
          resources: { sprites: sprites.size, visibleSprites, selectionSprites: Number(selection.visible), paletteMaterials: materials.length, materials: disposed ? 0 : materials.length + lineMaterials.length + 1, textures: disposed ? 0 : 2, geometries: disposed ? 0 : geometries.length + 1, rendererGeometries: renderer.info.memory.geometries, rendererTextures: renderer.info.memory.textures, disposed, lost }, backend: { ...backend } };
      },
      dispose() {
        if (disposed) return; clear(); disposed = true; controls.removeEventListener("change", changed); canvas.removeEventListener("webglcontextlost", contextLost); release();
      },
    };
  } catch (error) { release(); throw error; }
}
