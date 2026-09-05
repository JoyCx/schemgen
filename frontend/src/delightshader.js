// GLSL for the de-lit albedo and rejection-mask preview modes.
//
// This mirrors apply_delight / highlight_weight in backend/scripts/sample_colors.py.
// The whole point of the view is that it predicts what conversion will consume,
// so the constants below — specular exponent, de-light floor, clipping window,
// rejection ceiling — have to track the sampler's. If that model changes, this
// changes with it.
//
// Kept out of the component so it can be compiled against a real WebGL context
// in a test without standing up React and a GLTF load first.

export const VERTEX_SHADER = /* glsl */`
varying vec3 vNormal;
varying vec2 vUv;
uniform mat3 uNormalMatrix;

void main() {
  vNormal = normalize(uNormalMatrix * normal);
  vUv = uv;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}
`

export const FRAGMENT_SHADER = /* glsl */`
varying vec3 vNormal;
varying vec2 vUv;

uniform sampler2D uMap;
uniform bool uHasMap;
uniform vec3 uBaseColor;
uniform vec3 uLightDir;
uniform float uAmbient;
uniform float uGloss;
uniform float uSpecular;
uniform float uDelight;
uniform float uRejection;
uniform float uAlphaTest;
uniform int uMode;          // 0 = de-lit albedo, 1 = rejection mask

const float SPEC_EXPONENT = 16.0;
const float DELIGHT_FLOOR = 0.25;
const float MAX_REJECTION = 0.95;

vec3 linearToSrgb(vec3 c) {
  c = clamp(c, 0.0, 1.0);
  return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(vec3(0.0031308), c));
}

void main() {
  // An sRGB-tagged texture is decoded by the sampler hardware, so this is
  // linear light already — the same space the Python sampler averages in.
  vec3 observed = uBaseColor;
  float alpha = 1.0;
  if (uHasMap) {
    vec4 texel = texture2D(uMap, vUv);
    observed *= texel.rgb;
    alpha = texel.a;
  }
  // ShaderMaterial does not apply alphaTest for you, and a cutout texture read
  // as opaque would show the sampler discarding surface that is not there.
  if (alpha < uAlphaTest) discard;

  float ndl = clamp(dot(normalize(vNormal), uLightDir), 0.0, 1.0);
  float lobe = pow(ndl, SPEC_EXPONENT);
  float diffuse = uAmbient + (1.0 - uAmbient) * ndl;

  // White light adds to every channel, so the highlight comes off by
  // subtraction; the diffuse term scales the albedo, so it comes off by division.
  float specular = uDelight * uSpecular * uGloss * lobe;
  float divisor = max(1.0 + uDelight * (diffuse - 1.0), DELIGHT_FLOOR);
  vec3 albedo = max(observed - vec3(specular), vec3(0.0)) / divisor;

  if (uMode == 1) {
    float lobeTerm = uSpecular * uGloss * lobe;
    float frac = clamp(lobeTerm / max(diffuse + lobeTerm, 1e-6), 0.0, 1.0);
    float inLobe = smoothstep(0.02, 0.25, frac);
    float clipped = smoothstep(0.85, 0.99, max(max(observed.r, observed.g), observed.b));
    float discarded = uRejection * MAX_REJECTION
      * clamp(inLobe * (0.35 + 0.65 * clipped), 0.0, 1.0);
    float luma = dot(albedo, vec3(0.2126, 0.7152, 0.0722));
    vec3 ground = vec3(0.12 + 0.35 * luma);
    gl_FragColor = vec4(linearToSrgb(mix(ground, vec3(0.95, 0.16, 0.05), discarded)), alpha);
  } else {
    gl_FragColor = vec4(linearToSrgb(albedo), alpha);
  }
}
`
