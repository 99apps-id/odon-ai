/**
 * Polyfill for WebGL shader precision format and parameter lookup.
 *
 * In certain browser/webview environments (e.g., software renderers, SwiftShader,
 * virtualized displays, or buggy GPU drivers), `gl.getShaderPrecisionFormat(...)`
 * or `gl.getParameter(...)` returns `null`.
 *
 * Three.js WebGLCapabilities internally evaluates:
 *   gl.getShaderPrecisionFormat(gl.VERTEX_SHADER, gl.HIGH_FLOAT).precision > 0
 * And WebGLState evaluates:
 *   gl.getParameter(gl.VERSION).indexOf('WebGL')
 *
 * If either returns null, Three.js throws unhandled TypeErrors:
 *   - Cannot read properties of null (reading 'precision')
 *   - Cannot read properties of null (reading 'indexOf')
 *
 * This patch intercepts these calls when they return null and provides safe
 * fallback values so Three.js initializes gracefully without crashing.
 */
export function installWebGLPrecisionPolyfill(): void {
  if (typeof window === 'undefined') return;

  const patchPrototype = (proto: any) => {
    if (!proto) return;

    // 1. Guard getShaderPrecisionFormat
    const origPrecision = proto.getShaderPrecisionFormat;
    if (typeof origPrecision === 'function' && !(origPrecision as any).__odon_patched) {
      const patchedPrecision = function (this: any, shaderType: number, precisionType: number) {
        try {
          const res = origPrecision.call(this, shaderType, precisionType);
          if (res && typeof res.precision === 'number') {
            return res;
          }
        } catch {
          // Fall through to fallback
        }

        // gl.HIGH_FLOAT is 0x8DF2 (36338)
        const gl = this;
        const isHigh =
          (gl && typeof gl.HIGH_FLOAT === 'number' && precisionType === gl.HIGH_FLOAT) ||
          precisionType === 36338;

        if (isHigh) {
          return { rangeMin: 0, rangeMax: 0, precision: 0 };
        }
        return { rangeMin: 1, rangeMax: 1, precision: 14 };
      };

      (patchedPrecision as any).__odon_patched = true;
      proto.getShaderPrecisionFormat = patchedPrecision;
    }

    // 2. Guard getParameter for gl.VERSION and core state vectors
    const origGetParameter = proto.getParameter;
    if (typeof origGetParameter === 'function' && !(origGetParameter as any).__odon_patched) {
      const patchedGetParameter = function (this: any, pname: number) {
        try {
          const res = origGetParameter.call(this, pname);
          if (res !== null && res !== undefined) {
            return res;
          }
        } catch {
          // Fall through to fallback
        }

        switch (pname) {
          case 0x1f02: // gl.VERSION
            return 'WebGL 2.0 (OpenGL ES 3.0 Chromium)';
          case 0x1f00: // gl.VENDOR
            return 'WebKit';
          case 0x1f01: // gl.RENDERER
            return 'WebKit WebGL';
          case 0x8b36: // gl.SHADING_LANGUAGE_VERSION
            return 'WebGL GLSL ES 3.00 (OpenGL ES GLSL ES 3.0 Chromium)';
          case 0x8b4d: // gl.MAX_COMBINED_TEXTURE_IMAGE_UNITS
            return 16;
          case 0x0ba2: // gl.VIEWPORT
            return new Int32Array([0, 0, 800, 600]);
          case 0x0c10: // gl.SCISSOR_BOX
            return new Int32Array([0, 0, 800, 600]);
          case 0x0d33: // gl.MAX_TEXTURE_SIZE
            return 2048;
          case 0x8515: // gl.MAX_CUBE_MAP_TEXTURE_SIZE
            return 2048;
          case 0x8869: // gl.MAX_VERTEX_ATTRIBS
            return 16;
          case 0x8872: // gl.MAX_VERTEX_TEXTURE_IMAGE_UNITS
            return 16;
          case 0x8b4c: // gl.MAX_FRAGMENT_UNIFORM_VECTORS
            return 1024;
          case 0x8dfb: // gl.MAX_VARYING_VECTORS
            return 15;
          case 0x8df8: // gl.MAX_VERTEX_UNIFORM_VECTORS
            return 1024;
          default:
            return null;
        }
      };

      (patchedGetParameter as any).__odon_patched = true;
      proto.getParameter = patchedGetParameter;
    }
  };

  try {
    if (typeof WebGLRenderingContext !== 'undefined') {
      patchPrototype(WebGLRenderingContext.prototype);
    }
  } catch {}

  try {
    if (typeof WebGL2RenderingContext !== 'undefined') {
      patchPrototype(WebGL2RenderingContext.prototype);
    }
  } catch {}
}

// Auto-install on module import
installWebGLPrecisionPolyfill();
