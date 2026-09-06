use std::fs;
use std::time::Instant;

use candle::{D, DType, Device, Module, Tensor};
use candle_nn::{
    Embedding, LayerNorm, Linear, VarBuilder, embedding, layer_norm, linear, linear_no_bias,
};
use candle_transformers::models::jina_bert::{Config, PositionEmbeddingType};
use tokenizers::{TruncationParams, TruncationStrategy};

use crate::cache::{ModelPaths, inspect_cache};
use crate::constants::{MODEL_DIMENSIONS, MODEL_MAX_TOKENS};

pub struct LocalModel {
    model: JinaCodeModel,
    tokenizer: tokenizers::Tokenizer,
}

pub enum EmbedError {
    Inference(String),
}

pub struct EmbeddingOutput {
    pub values: Vec<f32>,
    pub inference_ms: u64,
    pub truncated: bool,
}

impl LocalModel {
    #[expect(
        unsafe_code,
        reason = "verified safetensors are memory-mapped through Candle's unsafe loader"
    )]
    pub fn load(paths: &ModelPaths) -> Result<Self, String> {
        let status = inspect_cache(paths);
        if !status.ready {
            return Err(status.problem.unwrap_or_else(|| {
                "the local model is unavailable; run `fallow similar-code setup --local`"
                    .to_string()
            }));
        }

        let config_bytes = fs::read(&paths.config)
            .map_err(|error| format!("failed to read model config: {error}"))?;
        let config: Config = serde_json::from_slice(&config_bytes)
            .map_err(|error| format!("failed to parse model config: {error}"))?;
        validate_config(&config)?;
        let mut tokenizer = tokenizers::Tokenizer::from_file(&paths.tokenizer)
            .map_err(|_| "failed to load the verified model tokenizer".to_string())?;
        if tokenizer.get_vocab_size(true) != config.vocab_size {
            return Err("the tokenizer vocabulary does not match the model config".to_string());
        }
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MODEL_MAX_TOKENS,
                strategy: TruncationStrategy::LongestFirst,
                stride: 0,
                ..TruncationParams::default()
            }))
            .map_err(|_| "failed to configure tokenizer truncation".to_string())?;

        let device = Device::Cpu;
        // SAFETY: setup pins, size-checks, and SHA-256 verifies this regular
        // safetensors file. It remains owned by the immutable revision cache
        // while the sidecar process holds the memory map.
        let variables = unsafe {
            VarBuilder::from_mmaped_safetensors(
                std::slice::from_ref(&paths.model),
                DType::F32,
                &device,
            )
        }
        .map_err(|error| format!("failed to open verified model weights: {error}"))?;
        let model = JinaCodeModel::load(&variables, &config)
            .map_err(|error| format!("failed to initialize JinaBERT: {error}"))?;
        Ok(Self { model, tokenizer })
    }

    pub fn embed(&self, source: &str) -> Result<EmbeddingOutput, EmbedError> {
        let encoding = self
            .tokenizer
            .encode(source, true)
            .map_err(|_| EmbedError::Inference("tokenization failed".to_string()))?;
        let ids = encoding.get_ids();
        if ids.len() > MODEL_MAX_TOKENS {
            return Err(EmbedError::Inference(
                "tokenizer exceeded the configured token limit".to_string(),
            ));
        }
        let attention_mask = encoding.get_attention_mask();
        let truncated = !encoding.get_overflowing().is_empty();

        let started = Instant::now();
        let input = Tensor::new(ids, self.model.device())
            .and_then(|tensor| tensor.unsqueeze(0))
            .map_err(candle_error)?;
        let token_embeddings = self.model.forward(&input).map_err(candle_error)?;
        let mask = Tensor::new(attention_mask, self.model.device())
            .and_then(|tensor| tensor.unsqueeze(0))
            .and_then(|tensor| tensor.unsqueeze(2))
            .and_then(|tensor| tensor.to_dtype(DType::F32))
            .map_err(candle_error)?;
        let masked = token_embeddings
            .broadcast_mul(&mask)
            .map_err(candle_error)?;
        let denominator = mask.sum(1).map_err(candle_error)?;
        let pooled = masked
            .sum(1)
            .and_then(|tensor| tensor.broadcast_div(&denominator))
            .map_err(candle_error)?;
        let norm = pooled
            .sqr()
            .and_then(|tensor| tensor.sum_keepdim(1))
            .and_then(|tensor| tensor.sqrt())
            .map_err(candle_error)?;
        let normalized = pooled.broadcast_div(&norm).map_err(candle_error)?;
        let rows = normalized.to_vec2::<f32>().map_err(candle_error)?;
        let values = rows
            .into_iter()
            .next()
            .ok_or_else(|| EmbedError::Inference("model returned no embedding".to_string()))?;
        if values.len() != MODEL_DIMENSIONS || values.iter().any(|value| !value.is_finite()) {
            return Err(EmbedError::Inference(
                "model returned an invalid embedding".to_string(),
            ));
        }
        Ok(EmbeddingOutput {
            values,
            inference_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            truncated,
        })
    }
}

fn validate_config(config: &Config) -> Result<(), String> {
    if config.hidden_size != MODEL_DIMENSIONS {
        return Err("the model config has an unexpected embedding width".to_string());
    }
    if config.position_embedding_type != PositionEmbeddingType::Alibi {
        return Err("the model config does not use the required ALiBi positions".to_string());
    }
    if !config
        .hidden_size
        .is_multiple_of(config.num_attention_heads)
    {
        return Err("the model attention width is invalid".to_string());
    }
    Ok(())
}

struct JinaCodeModel {
    embeddings: JinaEmbeddings,
    layers: Vec<JinaLayer>,
    alibi_slopes: Tensor,
    device: Device,
}

impl JinaCodeModel {
    fn load(vb: &VarBuilder<'_>, config: &Config) -> candle::Result<Self> {
        let embeddings = JinaEmbeddings::load(&vb.pp("embeddings"), config)?;
        let layers = (0..config.num_hidden_layers)
            .map(|index| JinaLayer::load(&vb.pp(format!("encoder.layer.{index}")), config))
            .collect::<candle::Result<Vec<_>>>()?;
        let alibi_slopes = alibi_slopes(config.num_attention_heads, vb.device())?;
        Ok(Self {
            embeddings,
            layers,
            alibi_slopes,
            device: vb.device().clone(),
        })
    }

    fn device(&self) -> &Device {
        &self.device
    }

    fn forward(&self, input_ids: &Tensor) -> candle::Result<Tensor> {
        let (_, sequence_length) = input_ids.dims2()?;
        let bias = alibi_bias(sequence_length, &self.alibi_slopes)?;
        let mut hidden = self.embeddings.forward(input_ids)?;
        for layer in &self.layers {
            hidden = layer.forward(&hidden, &bias)?;
        }
        Ok(hidden)
    }
}

struct JinaEmbeddings {
    word: Embedding,
    token_type: Embedding,
    layer_norm: LayerNorm,
}

impl JinaEmbeddings {
    fn load(vb: &VarBuilder<'_>, config: &Config) -> candle::Result<Self> {
        Ok(Self {
            word: embedding(
                config.vocab_size,
                config.hidden_size,
                vb.pp("word_embeddings"),
            )?,
            token_type: embedding(
                config.type_vocab_size,
                config.hidden_size,
                vb.pp("token_type_embeddings"),
            )?,
            layer_norm: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("LayerNorm"),
            )?,
        })
    }

    fn forward(&self, input_ids: &Tensor) -> candle::Result<Tensor> {
        let (batch_size, sequence_length) = input_ids.dims2()?;
        let words = self.word.forward(input_ids)?;
        let token_types = Tensor::zeros(sequence_length, DType::U32, input_ids.device())?
            .broadcast_left(batch_size)?
            .apply(&self.token_type)?;
        self.layer_norm.forward(&(&words + token_types)?)
    }
}

struct JinaAttention {
    query: Linear,
    key: Linear,
    value: Linear,
    query_norm: LayerNorm,
    key_norm: LayerNorm,
    output: Linear,
    output_norm: LayerNorm,
    heads: usize,
    head_size: usize,
}

impl JinaAttention {
    fn load(vb: &VarBuilder<'_>, config: &Config) -> candle::Result<Self> {
        let self_vb = vb.pp("self");
        let output_vb = vb.pp("output");
        Ok(Self {
            query: linear(config.hidden_size, config.hidden_size, self_vb.pp("query"))?,
            key: linear(config.hidden_size, config.hidden_size, self_vb.pp("key"))?,
            value: linear(config.hidden_size, config.hidden_size, self_vb.pp("value"))?,
            query_norm: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                self_vb.pp("layer_norm_q"),
            )?,
            key_norm: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                self_vb.pp("layer_norm_k"),
            )?,
            output: linear(
                config.hidden_size,
                config.hidden_size,
                output_vb.pp("dense"),
            )?,
            output_norm: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                output_vb.pp("LayerNorm"),
            )?,
            heads: config.num_attention_heads,
            head_size: config.hidden_size / config.num_attention_heads,
        })
    }

    fn forward(&self, input: &Tensor, bias: &Tensor) -> candle::Result<Tensor> {
        let query = self.project_and_transpose(input, &self.query, Some(&self.query_norm))?;
        let key = self.project_and_transpose(input, &self.key, Some(&self.key_norm))?;
        let value = self.project_and_transpose(input, &self.value, None)?;
        let head_size = u32::try_from(self.head_size)
            .map_err(|_| candle::Error::Msg("attention head width exceeds u32".to_string()))?;
        let scores = (query.matmul(&key.t()?)? / f64::from(head_size).sqrt())?;
        let probabilities = candle_nn::ops::softmax_last_dim(&scores.broadcast_add(bias)?)?;
        let context = probabilities
            .matmul(&value)?
            .transpose(1, 2)?
            .contiguous()?
            .flatten_from(D::Minus2)?;
        let projected = self.output.forward(&context)?;
        self.output_norm.forward(&(&projected + input)?)
    }

    fn project_and_transpose(
        &self,
        input: &Tensor,
        projection: &Linear,
        normalization: Option<&LayerNorm>,
    ) -> candle::Result<Tensor> {
        let projected = projection.forward(input)?;
        let projected =
            normalization.map_or(Ok(projected.clone()), |norm| norm.forward(&projected))?;
        let mut shape = projected.dims().to_vec();
        shape.pop();
        shape.push(self.heads);
        shape.push(self.head_size);
        projected.reshape(shape)?.transpose(1, 2)?.contiguous()
    }
}

struct JinaMlp {
    up_gated: Linear,
    down: Linear,
    intermediate_size: usize,
}

impl JinaMlp {
    fn load(vb: &VarBuilder<'_>, config: &Config) -> candle::Result<Self> {
        Ok(Self {
            up_gated: linear_no_bias(
                config.hidden_size,
                config.intermediate_size.saturating_mul(2),
                vb.pp("up_gated_layer"),
            )?,
            down: linear(
                config.intermediate_size,
                config.hidden_size,
                vb.pp("down_layer"),
            )?,
            intermediate_size: config.intermediate_size,
        })
    }

    fn forward(&self, input: &Tensor) -> candle::Result<Tensor> {
        let projected = self.up_gated.forward(input)?;
        let up = projected.narrow(D::Minus1, 0, self.intermediate_size)?;
        let gated = projected
            .narrow(D::Minus1, self.intermediate_size, self.intermediate_size)?
            .gelu_erf()?;
        self.down.forward(&(&up * &gated)?)
    }
}

struct JinaLayer {
    attention: JinaAttention,
    attention_norm: LayerNorm,
    mlp: JinaMlp,
    output_norm: LayerNorm,
}

impl JinaLayer {
    fn load(vb: &VarBuilder<'_>, config: &Config) -> candle::Result<Self> {
        Ok(Self {
            attention: JinaAttention::load(&vb.pp("attention"), config)?,
            attention_norm: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("layer_norm_1"),
            )?,
            mlp: JinaMlp::load(&vb.pp("mlp"), config)?,
            output_norm: layer_norm(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("layer_norm_2"),
            )?,
        })
    }

    fn forward(&self, input: &Tensor, bias: &Tensor) -> candle::Result<Tensor> {
        let attention = self.attention.forward(input, bias)?;
        let residual = self.attention_norm.forward(&(&attention + input)?)?;
        let mlp = self.mlp.forward(&residual)?;
        self.output_norm.forward(&(&residual + mlp)?)
    }
}

fn alibi_slopes(heads: usize, device: &Device) -> candle::Result<Tensor> {
    let mut power_of_two = 1usize;
    while power_of_two < heads {
        power_of_two = power_of_two.saturating_mul(2);
    }
    let power = u32::try_from(power_of_two)
        .map_err(|_| candle::Error::Msg("attention head count exceeds u32".to_string()))?;
    let all = (1..=power_of_two)
        .map(|value| {
            let value = u32::try_from(value).unwrap_or(u32::MAX);
            -1_f64 / 2_f64.powf(f64::from(value.saturating_mul(8)) / f64::from(power))
        })
        .collect::<Vec<_>>();
    let slopes = if power_of_two == heads {
        all
    } else {
        all.iter()
            .skip(1)
            .step_by(2)
            .chain(all.iter().step_by(2))
            .take(heads)
            .copied()
            .collect()
    };
    Tensor::new(slopes, device)?
        .to_dtype(DType::F32)?
        .reshape((1, heads, 1, 1))
}

fn alibi_bias(sequence_length: usize, slopes: &Tensor) -> candle::Result<Tensor> {
    let sequence_length_u32 = u32::try_from(sequence_length)
        .map_err(|_| candle::Error::Msg("sequence length exceeds u32".to_string()))?;
    let positions =
        Tensor::arange(0_u32, sequence_length_u32, slopes.device())?.to_dtype(DType::F32)?;
    let left = positions.reshape((1, sequence_length))?;
    let right = positions.reshape((sequence_length, 1))?;
    let distances =
        left.broadcast_sub(&right)?
            .abs()?
            .reshape((1, 1, sequence_length, sequence_length))?;
    distances.broadcast_mul(slopes)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Candle errors are owned at the API boundary"
)]
fn candle_error(error: candle::Error) -> EmbedError {
    EmbedError::Inference(format!("local model inference failed: {error}"))
}
