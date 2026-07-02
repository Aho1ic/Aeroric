/* global Path2D, DOMMatrix, document, window, requestAnimationFrame, cancelAnimationFrame, performance, ResizeObserver, IntersectionObserver */

let canvasRef = { value: null };
let reducedMotion = false;
let animationFrame = 0;
let resizeObserver;
let intersectionObserver;

// 调色板定义
const textureColors = ["#D97757", "#E08B6E", "#DD8263", "#E29478", "#D17A60", "#E6A085"];
const cellSolidColors = ["#CE6C4C", "#C9694C", "#D17052", "#CB6B4E"];

// 伪随机数发生器
function createSeededRandom(seed) {
  let seedVal = seed | 0;
  return () => {
    let t = Math.imul((seedVal = (seedVal + 0x6d2b79f5) | 0) ^ (seedVal >>> 15), 1 | seedVal);
    return (((t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t) ^ (t >>> 14)) >>> 0) / 0x100000000;
  };
}

function get2DContext(canvas, willReadFrequently = false) {
  return canvas.getContext("2d", { willReadFrequently });
}

// 标志物路径
const logoPathStr = "M18.3658 62.2435L36.7823 51.9165L37.0858 51.012L36.7823 50.5083H35.8716L32.7853 50.3206L22.2616 50.0389L13.1546 49.6634L4.30054 49.194L2.07438 48.7246L0 45.9551L0.202378 44.5938L2.07438 43.3264L4.75589 43.5611L10.6755 43.9836L19.5801 44.5938L26.0056 44.9693L35.568 45.9551H37.0858L37.2882 45.3448L36.7823 44.9693L36.3775 44.5938L27.1693 38.3507L17.2022 31.7789L11.9909 27.9767L9.20822 26.0522L7.79157 24.2684L7.18443 20.3254L9.71416 17.5089L13.1546 17.7436L14.0147 17.9783L17.5057 20.654L24.9431 26.4277L34.6573 33.5627L36.0739 34.7362L36.6444 34.3512L36.7317 34.079L36.0739 32.9994L30.8121 23.4704L25.1961 13.7537L22.6664 9.71675L22.0086 7.32277C21.7539 6.31812 21.6039 5.48695 21.6039 4.45938L24.4878 0.516349L26.1068 0L30.0026 0.516349L31.6216 1.92457L34.0502 7.46359L37.9459 16.1476L44.0173 27.9767L45.7881 31.4973L46.7494 34.7362L47.1036 35.722H47.7107V35.1587L48.2166 28.4931L49.1274 20.3254L50.0381 9.81063L50.3416 6.85336L51.8089 3.28586L54.7434 1.36128L57.0201 2.44092L58.8921 5.11655L58.6391 6.85336L57.5261 14.0822L55.3505 25.395L53.9338 32.9994H54.7434L55.7047 32.0136L59.5498 26.944L65.9753 18.8702L68.8086 15.6782L72.1479 12.1577L74.2729 10.4678H78.3204L81.2549 14.8802L79.9395 19.4335L75.7907 24.6909L72.3503 29.1503L67.4173 35.7593L64.3563 41.0732L64.6308 41.5116L65.3682 41.4487L76.499 39.0548L82.5198 37.9751L89.7042 36.7547L92.9423 38.2568L93.2964 39.8058L92.0316 42.9509L84.3412 44.8285L75.3354 46.6592L61.9245 49.8162L61.776 49.9356L61.9513 50.1956L67.9991 50.743L70.5795 50.8839H76.9038L88.6923 51.7757L91.7786 53.7942L93.6 56.282L93.2964 58.2066L88.5405 60.6006L82.1656 59.0985L67.2402 55.531L62.1302 54.2636H61.4218V54.6861L65.6718 58.8638L73.514 65.9049L83.2787 75.0114L83.7846 77.2646L82.5198 79.0483L81.2043 78.8606L72.6032 72.3827L69.264 69.4724L61.776 63.1354H61.2701V63.7926L62.9903 66.3274L72.1479 80.081L72.6032 84.3057L71.9455 85.667L69.5676 86.5119L66.9872 86.0425L61.5736 78.4851L56.0588 70.0357L51.6065 62.4313L51.0687 62.7708L48.419 91.0652L47.2048 92.5204L44.3715 93.6L41.9935 91.8162L40.7286 88.9059L41.9935 83.1322L43.5114 75.6217L44.7256 69.6602L45.8387 62.2435L46.5185 59.7659L46.4584 59.6001L45.9153 59.6914L40.3239 67.3601L31.824 78.8606L25.0949 86.0425L23.4759 86.6997L20.6932 85.2445L20.9462 82.6628L22.5146 80.3627L31.824 68.5336L37.44 61.1639L41.0595 56.9335L41.0243 56.3216L40.8245 56.3046L16.0891 72.4297L11.6874 72.993L9.76476 71.2092L10.0177 68.2989L10.9284 67.3601L18.3658 62.2435Z";

let logoCells = [];
let adjacencyMap = new Map();
let maxConnectedSize = 0;

// 初始化检测网格
function initializeLogoGrid() {
  const path2D = new Path2D(logoPathStr);
  const scaleMatrix = new DOMMatrix().scale(1 / 94, 1 / 94);
  const pathScaled = new Path2D();
  pathScaled.addPath(path2D, scaleMatrix);

  const testCanvas = document.createElement("canvas");
  testCanvas.width = testCanvas.height = 152;
  const ctx = get2DContext(testCanvas, true);

  ctx.save();
  ctx.scale(152, 152);
  ctx.fillStyle = "#000";
  ctx.fill(pathScaled);
  ctx.globalCompositeOperation = "destination-out";
  ctx.lineWidth = 0.025;
  ctx.lineJoin = "round";
  ctx.strokeStyle = "#000";
  ctx.stroke(pathScaled);
  ctx.restore();

  const imgData = ctx.getImageData(0, 0, 152, 152).data;
  const cellsFound = [];

  for (let y = 0; y < 19; y++) {
    for (let x = 0; x < 19; x++) {
      let opaqueCount = 0;
      for (let dy = 0; dy < 8; dy++) {
        for (let dx = 0; dx < 8; dx++) {
          const index = ((8 * y + dy) * 152 + (8 * x + dx)) * 4 + 3;
          if (imgData[index] > 8) {
            opaqueCount++;
          }
        }
      }
      if (opaqueCount >= 11.52) {
        const dx = x - 9;
        const dy = y - 9;
        cellsFound.push({
          idx: 19 * y + x,
          gx: x,
          gy: y,
          dist: Math.hypot(dx, dy)
        });
      }
    }
  }

  // 按照距中心位置排序
  cellsFound.sort((a, b) => a.dist - b.dist || a.idx - b.idx);
  logoCells = cellsFound;

  const cellsIndexSet = new Set(cellsFound.map(c => c.idx));
  adjacencyMap = new Map();

  for (const { idx, gx, gy } of cellsFound) {
    const neighbors = [];
    for (const [ox, oy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
      const nx = gx + ox;
      const ny = gy + oy;
      if (nx < 0 || ny < 0 || nx >= 19 || ny >= 19) continue;
      const nIdx = 19 * ny + nx;
      if (cellsIndexSet.has(nIdx)) {
        neighbors.push(nIdx);
      }
    }
    adjacencyMap.set(idx, neighbors);
  }

  // 计算最大强连通性尺寸
  const visited = new Set([180]);
  const queue = [180];
  while (queue.length) {
    const current = queue.shift();
    const neighbors = adjacencyMap.get(current) || [];
    for (const n of neighbors) {
      if (!visited.has(n)) {
        visited.add(n);
        queue.push(n);
      }
    }
  }
  maxConnectedSize = visited.size;
}

// 邻居邻接检索
function getNeighbors(idx) {
  return adjacencyMap.get(idx) || [];
}

// 粒子纹理生成
function generateTexture(index) {
  const rand = createSeededRandom(9301 * index + 49297);
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 128;
  const ctx = get2DContext(canvas);

  // 1. 基底背景填充
  ctx.globalAlpha = 0.88;
  ctx.fillStyle = textureColors[(rand() * textureColors.length) | 0];
  ctx.fillRect(0, 0, 128, 128);

  // 2. 5层有机渐变斑点
  for (let i = 0; i < 5; i++) {
    const cx = 128 * rand();
    const cy = 128 * rand();
    const radius = 128 * (0.25 + 0.35 * rand());
    const grad = ctx.createRadialGradient(cx, cy, 0, cx, cy, radius);
    const spotColor = textureColors[(rand() * textureColors.length) | 0];
    grad.addColorStop(0, spotColor);
    grad.addColorStop(1, "rgba(0,0,0,0)");
    ctx.globalAlpha = 0.14 + 0.14 * rand();
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, 128, 128);
  }

  // 3. 220个背景微弱噪点
  ctx.globalAlpha = 0.06;
  for (let i = 0; i < 220; i++) {
    ctx.fillStyle = textureColors[(rand() * textureColors.length) | 0];
    const size = 1 + 2 * rand();
    ctx.fillRect(128 * rand(), 128 * rand(), size, size);
  }

  // 4. 浮雕发光边框绘制
  ctx.globalAlpha = 0.14;
  ctx.strokeStyle = "#CF6E50";
  ctx.lineWidth = 6.4;
  ctx.shadowColor = "#CF6E50";
  ctx.shadowBlur = 12.8;
  ctx.beginPath();
  ctx.roundRect(8.96, 8.96, 110.08, 110.08, 25.6);
  ctx.stroke();
  ctx.shadowBlur = 0;

  // 5. 极坐标裁切
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = "destination-in";
  ctx.fillStyle = "#000";

  const phase1 = rand() * Math.PI * 2;
  const phase2 = rand() * Math.PI * 2;
  const phase3 = rand() * Math.PI * 2;

  ctx.beginPath();
  for (let i = 0; i <= 56; i++) {
    const theta = (i / 56) * Math.PI * 2;
    const cosVal = Math.cos(theta);
    const sinVal = Math.sin(theta);
    const rad = (1 / Math.pow(Math.abs(cosVal) ** 4 + Math.abs(sinVal) ** 4, 0.25)) * 58.88 *
      (1 + 0.03 * Math.sin(3 * theta + phase1) +
        0.018 * Math.sin(7 * theta + phase2) +
        0.01 * Math.sin(11 * theta + phase3));

    const x = 64 + cosVal * rad;
    const y = 64 + sinVal * rad;
    if (i === 0) {
      ctx.moveTo(x, y);
    } else {
      ctx.lineTo(x, y);
    }
  }
  ctx.closePath();
  ctx.fill();

  ctx.globalCompositeOperation = "source-over";
  return canvas;
}

// 纹理库及MIPMAP缓存
const textureCanvases = [];
function initializeTextureLibrary() {
  textureCanvases.length = 0;
  for (let i = 0; i < 12; i++) {
    textureCanvases.push(generateTexture(i + 1));
  }
}

// 生成MIPMAP缩图多级缓存
function generateMipmaps(originalCanvas) {
  const mipmaps = [originalCanvas];
  let current = originalCanvas;
  while (current.width > 38) {
    const targetSize = Math.max(19, current.width >> 1);
    const mip = document.createElement("canvas");
    mip.width = mip.height = targetSize;
    const ctx = get2DContext(mip);
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(current, 0, 0, targetSize, targetSize);
    mipmaps.push(mip);
    current = mip;
  }
  return mipmaps;
}

// 获取匹配尺寸的MIPMAP
function getMipmapForSize(mipmaps, minWidth) {
  for (let i = mipmaps.length - 1; i >= 0; i--) {
    if (mipmaps[i].width >= minWidth) {
      return mipmaps[i];
    }
  }
  return mipmaps[0];
}

// 生成翻转态/扩散形态的Canvas
function generateFrameCanvas(seed, canvasSize, adjacencyList, visibleCount) {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = canvasSize;
  const ctx = get2DContext(canvas);

  const step = canvasSize / 19;
  const size = step * 0.78;
  const offset = (step - size) / 2;

  for (let i = 0; i < visibleCount; i++) {
    const { gx, gy } = adjacencyList[i];
    const randSeed = ((0x466f45d * gx) ^ (0x127409f * gy) ^ (0x4f9ffb7 * seed)) >>> 0;
    const tex = textureCanvases[randSeed % 12];

    ctx.save();
    ctx.translate(gx * step + offset + size / 2, gy * step + offset + size / 2);
    ctx.scale((randSeed >> 4) & 1 ? -1 : 1, (randSeed >> 5) & 1 ? -1 : 1);
    ctx.drawImage(tex, -size / 2, -size / 2, size, size);
    ctx.restore();
  }

  // 终态圆角裁切
  ctx.globalCompositeOperation = "destination-in";
  ctx.fillStyle = "#000";
  ctx.beginPath();
  ctx.roundRect(0, 0, canvasSize, canvasSize, 0.18 * canvasSize);
  ctx.fill();

  return canvas;
}

// 制作单元实心块Canvas
function generateSolidCanvas(seed) {
  const rand = createSeededRandom(0x165667b1 * seed >>> 0);
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 152;
  const ctx = get2DContext(canvas);

  ctx.beginPath();
  ctx.roundRect(0, 0, 152, 152, 27.36);
  ctx.fillStyle = cellSolidColors[seed % cellSolidColors.length];
  ctx.fill();

  ctx.save();
  ctx.clip();
  for (let i = 0; i < 4; i++) {
    const cx = 152 * rand();
    const cy = 152 * rand();
    const r = 152 * (0.35 + 0.35 * rand());
    const grad = ctx.createRadialGradient(cx, cy, 0, cx, cy, r);
    grad.addColorStop(0, cellSolidColors[Math.floor(rand() * cellSolidColors.length)]);
    grad.addColorStop(1, "rgba(0,0,0,0)");
    ctx.globalAlpha = 0.22;
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, 152, 152);
  }
  ctx.restore();

  return canvas;
}

// 网格种子元素库组
const cellLibraries = [];
function initializeCellLibraries() {
  cellLibraries.length = 0;
  for (let i = 0; i < 8; i++) {
    const seed = i + 1;
    // 1. 生成基于广度优先搜索扩散的粒子列表
    const gridAdjacency = (() => {
      const rand = createSeededRandom(0x9e3779b1 * seed >>> 0);
      const getIndex = (x, y) => 19 * y + x;
      const visited = new Set();
      const cellsSequence = [];

      const addCell = (x, y) => {
        const idx = getIndex(x, y);
        if (!visited.has(idx)) {
          visited.add(idx);
          cellsSequence.push({ gx: x, gy: y });
          return true;
        }
        return false;
      };

      addCell(9, 9);
      const subPathsCount = 5 + Math.floor(3 * rand());
      for (let p = 0; p < subPathsCount; p++) {
        let cx = 9;
        let cy = 9;
        for (let step = 0; step < 38; step++) {
          const options = [[1, 0], [-1, 0], [0, 1], [0, -1]].filter(([ox, oy]) => {
            const nx = cx + ox;
            const ny = cy + oy;
            return nx >= 0 && ny >= 0 && nx < 19 && ny < 19 && !visited.has(getIndex(nx, ny));
          });
          if (!options.length) break;

          let target = options[0];
          let maxDist = -Infinity;
          for (const opt of options) {
            const dist = Math.hypot(cx + opt[0] - 9, cy + opt[1] - 9) + 1.2 * rand();
            if (dist > maxDist) {
              maxDist = dist;
              target = opt;
            }
          }
          cx += target[0];
          cy += target[1];
          addCell(cx, cy);
          if (cx === 0 || cy === 0 || cx === 18 || cy === 18) break;
        }
      }

      // 获取当前节点周围的备用点
      const adjacents = [];
      const getAdjacents = (cx, cy) => {
        for (const [ox, oy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
          const nx = cx + ox;
          const ny = cy + oy;
          if (nx < 0 || ny < 0 || nx >= 19 || ny >= 19) continue;
          if (!visited.has(getIndex(nx, ny))) {
            adjacents.push({ gx: nx, gy: ny });
          }
        }
      };

      for (const { gx, gy } of cellsSequence) {
        getAdjacents(gx, gy);
      }

      while (cellsSequence.length < 361) {
        let chosen;
        do {
          const pickIndex = Math.floor(rand() * adjacents.length);
          chosen = adjacents[pickIndex];
          if (chosen) {
            adjacents[pickIndex] = adjacents[adjacents.length - 1];
            adjacents.pop();
          }
        } while (chosen && visited.has(getIndex(chosen.gx, chosen.gy)));

        if (!chosen) break;
        addCell(chosen.gx, chosen.gy);
        getAdjacents(chosen.gx, chosen.gy);
      }
      return cellsSequence;
    })();

    // 2. 生成多级大尺度翻转 canvas
    const baseCanvas = generateFrameCanvas(seed, 608, gridAdjacency, 361);
    const mipmaps = generateMipmaps(baseCanvas);

    // 3. 生成 8 组翻转分阶段帧集
    const flips = [];
    for (let f = 0; f < 8; f++) {
      const visibleCount = Math.ceil((f + 1) / 8 * 361);
      flips.push(generateFrameCanvas(seed, 152, gridAdjacency, visibleCount));
    }

    // 4. 实心组底色Canvas
    const solid = generateSolidCanvas(seed);

    cellLibraries.push({
      full: mipmaps,
      flip: flips,
      solid: solid
    });
  }
}

// 组装整体大画布
let mainGridMipmaps = [];
function initializeMainGrid() {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 608;
  const ctx = get2DContext(canvas);

  const step = 32;
  const size = 24.96;

  for (let i = 0; i < logoCells.length; i++) {
    const { gx, gy, idx } = logoCells[i];
    const cellObj = cellLibraries[idx % 8];
    const x = gx * step + (step - size) / 2;
    const y = gy * step + (step - size) / 2;
    ctx.drawImage(cellObj.solid, x, y, size, size);
  }

  mainGridMipmaps = generateMipmaps(canvas);
}

// 模拟状态机
const seedRandom = createSeededRandom(0xc1a0de);
function createSimulationState(bornTime) {
  return {
    born: bornTime,
    cells: new Map(), // 记录已激活单元格 idx -> 激活时刻
    agents: [],       // 活跃 agent 列表
    lastTick: bornTime,
    ticks: 0,
    doneAt: 0
  };
}

function spawnAgent(state, sinceTime, isImmediate = false) {
  state.agents.push({
    idx: 180,
    prev: 180,
    since: sinceTime,
    bornAt: isImmediate ? -Infinity : sinceTime,
    dieAt: 0
  });
}

// 物理和绘制变量
let ctxMain = null;
let dpr = 1;
let width = 0;
let height = 0;
let scaleCenterX = 0;
let scaleCenterY = 0;
let scaleBase = 0;

// 初始化画布尺寸
function updateCanvasLayout() {
  const canvas = canvasRef.value;
  if (!canvas) return;

  dpr = Math.min(2, window.devicePixelRatio || 1);
  width = canvas.clientWidth;
  height = canvas.clientHeight;
  canvas.width = Math.round(width * dpr);
  canvas.height = Math.round(height * dpr);

  ctxMain = get2DContext(canvas);
  ctxMain.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctxMain.imageSmoothingEnabled = true;
  ctxMain.imageSmoothingQuality = "high";

  const isWide = width >= 992;
  scaleCenterX = isWide ? width * 0.66 : width * 0.5;
  scaleCenterY = isWide ? height * 0.5 : height * 0.62;
  scaleBase = isWide
    ? 0.58 * Math.min(0.6 * width, height)
    : 0.5 * Math.min(0.92 * width, 0.62 * height);
}

// 绘制网格背景线
function drawBackgroundLines(cx, cy, gap, opacity) {
  if (gap < 2 || (width / gap + 2) * (height / gap + 2) > 12000) return;

  const progress = Math.max(0, Math.min(1, (gap - 2) / 7));
  const lineAlpha = (0.45 + 0.45 * Math.min(1, gap / 90)) * opacity * progress;
  if (lineAlpha < 0.01) return;

  const startX = ((cx + gap / 2) % gap + gap) % gap;
  const startY = ((cy + gap / 2) % gap + gap) % gap;

  ctxMain.fillStyle = "#C9C6BC";
  ctxMain.globalAlpha = lineAlpha;
  for (let y = startY; y <= height; y += gap) {
    for (let x = startX; x <= width; x += gap) {
      ctxMain.fillRect(x - 1, y - 1, 2, 2);
    }
  }
  ctxMain.globalAlpha = 1;
}

// 网格位置换算
function getCellPosition(idx, cx, cy, stepSize, logoScale) {
  return [
    cx + 2 * stepSize * (idx % 19 - logoScale),
    cy + 2 * stepSize * (Math.floor(idx / 19) - logoScale)
  ];
}

const scaleExponentBase = Math.log(19 / 0.78);
let springPosition = 0;
let springVelocity = 0;
let simLayer = 0;
let lastTickTime = 0;
let animationRunning = false;
let activeSimState = null;

export function shouldRestartRecursiveHeroLoop({
  isFinalLayer,
  doneAt,
  currentTime,
  springVelocity: velocity
}) {
  return Boolean(
    isFinalLayer &&
      doneAt &&
      currentTime >= doneAt &&
      Math.abs(velocity) < 1e-5
  );
}

function restartSimulationCycle(currentTime) {
  activeSimState = createSimulationState(currentTime);
  spawnAgent(activeSimState, currentTime, false);
  simLayer = 0;
  springPosition = -0.04 * scaleExponentBase;
  springVelocity = 0;
  lastTickTime = currentTime;
}

// 物理阻尼模拟与动画绘制主循环
function renderSimulation(currentTime) {
  let deltaTime = Math.min(50, currentTime - lastTickTime);
  lastTickTime = currentTime;

  // 1. 模拟状态推进 (每 450ms 进行一次物理步进)
  if (!activeSimState.doneAt) {
    while (currentTime - activeSimState.lastTick >= 450) {
      activeSimState.lastTick += 450;
      activeSimState.ticks++;
      const currentTickTime = activeSimState.lastTick;

      const activeIndices = new Set(activeSimState.agents.filter(a => !a.dieAt).map(a => a.idx));
      const nextAgents = [];

      for (const agent of activeSimState.agents) {
        if (agent.dieAt) {
          if (currentTickTime - agent.dieAt < 350) {
            nextAgents.push(agent);
          }
          continue;
        }

        // 寻找连通方向的邻居
        const viableNeighbors = getNeighbors(agent.idx).filter(
          nIdx => !activeSimState.cells.has(nIdx) && !activeIndices.has(nIdx)
        );

        activeSimState.cells.set(agent.idx, currentTickTime);

        if (!viableNeighbors.length) {
          agent.dieAt = currentTickTime;
          nextAgents.push(agent);
          continue;
        }

        // 随机抉择扩散邻居
        const nextIdx = viableNeighbors[Math.floor(seedRandom() * viableNeighbors.length)];
        agent.prev = agent.idx;
        agent.idx = nextIdx;
        agent.since = currentTickTime;
        activeIndices.add(nextIdx);
        nextAgents.push(agent);
      }
      activeSimState.agents = nextAgents;

      const aliveCount = activeSimState.agents.filter(a => !a.dieAt).length;

      // 寻找细胞四周已填充但外围还有的节点
      const outerUnfilled = (() => {
        const list = [];
        const checked = new Set();
        for (const idx of activeSimState.cells.keys()) {
          for (const n of getNeighbors(idx)) {
            if (!activeSimState.cells.has(n) && !checked.has(n)) {
              checked.add(n);
              list.push(n);
            }
          }
        }
        return list;
      })().filter(idx => !activeIndices.has(idx));

      if (!outerUnfilled.length && !aliveCount) {
        activeSimState.doneAt = currentTickTime;
        break;
      }

      // 根据状态步数动态增加 Agent 加速扩散
      const tickCap = 7 / (1 + 0.25 * simLayer);
      const allowedCount = Math.min(48, Math.max(1, Math.floor((activeSimState.ticks / tickCap) ** 2)));
      let currentAlive = aliveCount;

      while (currentAlive < allowedCount && outerUnfilled.length) {
        const randIndex = Math.floor(seedRandom() * outerUnfilled.length);
        const spawnIdx = outerUnfilled[randIndex];
        outerUnfilled[randIndex] = outerUnfilled[outerUnfilled.length - 1];
        outerUnfilled.pop();

        activeSimState.agents.push({
          idx: spawnIdx,
          prev: spawnIdx,
          since: currentTickTime,
          bornAt: currentTickTime,
          dieAt: 0
        });
        activeIndices.add(spawnIdx);
        currentAlive++;
      }
    }
  }

  // 2. 状态重置与级联放大递进
  const isFinalLayer = simLayer + 1 >= 3;
  if (activeSimState.doneAt && currentTime - activeSimState.doneAt >= 500 && !isFinalLayer) {
    activeSimState = createSimulationState(currentTime);
    activeSimState.lastTick = currentTime;
    spawnAgent(activeSimState, currentTime, true);
    simLayer++;
  }

  // 3. 弹簧阻尼物理公式更新
  const progressRatio = activeSimState.cells.size / maxConnectedSize;
  const isDonePhase = activeSimState.doneAt && !isFinalLayer;
  const currentTargetExponent = simLayer + (isDonePhase ? 1 : -0.04 + 0.34 * progressRatio);
  const springTarget = currentTargetExponent * scaleExponentBase;

  const steps = Math.max(1, Math.ceil(deltaTime / 12));
  const subDt = deltaTime / steps;
  for (let i = 0; i < steps; i++) {
    const force = 81e-8 * (springTarget - springPosition) - 0.00153 * springVelocity;
    springVelocity += force * subDt;
    springPosition += springVelocity * subDt;
  }

  // 4. 动画图形渲染
  ctxMain.fillStyle = "#FAF9F5";
  ctxMain.fillRect(0, 0, width, height);

  const zoomFactor = scaleBase * Math.exp(simLayer * scaleExponentBase - springPosition);

  // 绘制粒子和网格背景
  drawBackgroundLines(scaleCenterX, scaleCenterY, (zoomFactor / 19) * 2, 1);
  drawBackgroundLines(scaleCenterX, scaleCenterY, (0.78 * zoomFactor / 19) / 19, 1);

  // 绘制网格单元
  (() => {
    const stepSize = zoomFactor / 19;
    if (stepSize < 0.4) return;
    const particleSize = 2 * stepSize * 0.78;

    for (const [idx, activeTime] of activeSimState.cells) {
      const [cellX, cellY] = getCellPosition(idx, scaleCenterX, scaleCenterY, stepSize, 9);
      const cellObj = cellLibraries[idx % 8];
      const age = currentTime - activeTime;

      if (age < 700) {
        const frame = Math.min(7, Math.floor((age / 700) * 8));
        ctxMain.drawImage(cellObj.flip[frame], cellX - particleSize / 2, cellY - particleSize / 2, particleSize, particleSize);
      } else {
        const fadeAge = age - 700;
        const opacity = Math.min(1, fadeAge / 1600);
        if (opacity < 1) {
          ctxMain.drawImage(
            getMipmapForSize(cellObj.full, particleSize * dpr),
            cellX - particleSize / 2,
            cellY - particleSize / 2,
            particleSize,
            particleSize
          );
        }
        if (opacity > 0) {
          ctxMain.globalAlpha = opacity;
          ctxMain.drawImage(cellObj.solid, cellX - particleSize / 2, cellY - particleSize / 2, particleSize, particleSize);
          ctxMain.globalAlpha = 1;
        }
      }
    }
  })();

  // 绘制活跃 Agent (跑动粒子)
  (() => {
    const stepSize = zoomFactor / 19;
    if (stepSize < 0.4) return;

    for (const agent of activeSimState.agents) {
      let animProgress = Math.min(1, (currentTime - agent.since) / 450);
      let smoothProgress = 1 - (1 - animProgress) * (1 - animProgress);

      const [prevX, prevY] = getCellPosition(agent.prev, scaleCenterX, scaleCenterY, stepSize, 9);
      const [nextX, nextY] = getCellPosition(agent.idx, scaleCenterX, scaleCenterY, stepSize, 9);

      const currX = prevX + (nextX - prevX) * smoothProgress;
      const currY = prevY + (nextY - prevY) * smoothProgress;

      let opacity = 1;
      if (agent.dieAt) {
        opacity = Math.pow(1 - Math.min(1, (currentTime - agent.dieAt) / 350), 2);
      } else if (agent.bornAt !== -Infinity) {
        opacity = 1 - Math.pow(1 - Math.min(1, (currentTime - agent.bornAt) / 350), 3);
      }

      if (opacity <= 0) continue;

      const size = stepSize * opacity;
      ctxMain.drawImage(
        getMipmapForSize(mainGridMipmaps, 2 * size * 0.78 * dpr),
        currX - size * 0.78,
        currY - size * 0.78,
        2 * size * 0.78,
        2 * size * 0.78
      );
    }
  })();

  if (
    shouldRestartRecursiveHeroLoop({
      isFinalLayer,
      doneAt: activeSimState.doneAt,
      currentTime,
      springVelocity
    })
  ) {
    restartSimulationCycle(currentTime);
  }

  if (animationRunning) {
    animationFrame = requestAnimationFrame(renderSimulation);
  }
}

// 减弱动态模式 (PREFERS-REDUCED-MOTION)
function renderStaticState() {
  const canvas = canvasRef.value;
  if (!canvas) return;

  updateCanvasLayout();

  ctxMain.fillStyle = "#FAF9F5";
  ctxMain.fillRect(0, 0, width, height);

  // 绘制完全填满的静态标志
  const stepSize = scaleBase / 19;
  const particleSize = 2 * stepSize * 0.78;

  for (let i = 0; i < logoCells.length; i++) {
    const { gx: _gx, gy: _gy, idx } = logoCells[i];
    const [cellX, cellY] = getCellPosition(idx, scaleCenterX, scaleCenterY, stepSize, 9);
    const cellObj = cellLibraries[idx % 8];
    ctxMain.drawImage(cellObj.solid, cellX - particleSize / 2, cellY - particleSize / 2, particleSize, particleSize);
  }
}

// 动画生命周期控制
function startAnimation() {
  if (reducedMotion) {
    renderStaticState();
    return;
  }

  const now = performance.now();
  restartSimulationCycle(now);
  animationRunning = true;

  cancelAnimationFrame(animationFrame);
  animationFrame = requestAnimationFrame(renderSimulation);
}

function stopAnimation() {
  animationRunning = false;
  cancelAnimationFrame(animationFrame);
}

function mountRecursiveHeroEffect(canvas, options = {}) {
  if (!canvas) {
    throw new Error("createRecursiveHeroEffect requires a canvas element.");
  }

  canvasRef.value = canvas;
  reducedMotion = Boolean(options.reducedMotion);

  // 1. 初始化图形资源
  initializeLogoGrid();
  initializeTextureLibrary();
  initializeCellLibraries();
  initializeMainGrid();

  updateCanvasLayout();

  // 2. 布局变化监听
  resizeObserver = new ResizeObserver(() => {
    updateCanvasLayout();
    if (reducedMotion) {
      renderStaticState();
    }
  });
  if (canvasRef.value) resizeObserver.observe(canvasRef.value);

  // 3. 可见性监听，仅可见时运行
  intersectionObserver = new IntersectionObserver(([entry]) => {
    if (entry.isIntersecting) {
      startAnimation();
    } else {
      stopAnimation();
    }
  }, { threshold: 0.1 });
  if (canvasRef.value) intersectionObserver.observe(canvasRef.value);
}

function destroyRecursiveHeroEffect() {
  animationRunning = false;
  cancelAnimationFrame(animationFrame);
  resizeObserver?.disconnect();
  intersectionObserver?.disconnect();
  resizeObserver = undefined;
  intersectionObserver = undefined;
  ctxMain = null;
  canvasRef.value = null;
}

function setRecursiveHeroReducedMotion(isReduced) {
  reducedMotion = Boolean(isReduced);
  if (reducedMotion) {
    stopAnimation();
    renderStaticState();
  } else {
    startAnimation();
  }
}

export function createRecursiveHeroEffect(canvas, options = {}) {
  mountRecursiveHeroEffect(canvas, options);

  return {
    setReducedMotion: setRecursiveHeroReducedMotion,
    destroy: destroyRecursiveHeroEffect
  };
}
