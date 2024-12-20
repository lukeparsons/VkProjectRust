use crate::graphics::errors::IOResultToResultExt;
use crate::graphics::presentation::SwapchainSettings;
use crate::graphics::vk_app;
use crate::graphics::vk_app::Result;
use ash::vk;
use std::mem::offset_of;

pub(crate) struct Pipeline
{
    pub render_pass:           vk::RenderPass,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    pub pipeline_layout:       vk::PipelineLayout,
    pub graphics_pipeline:     vk::Pipeline,
}

impl Pipeline
{
    pub fn cleanup(&self, device: &ash::Device)
    {
        unsafe {
            device.destroy_descriptor_set_layout(self.descriptor_set_layout, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
            device.destroy_render_pass(self.render_pass, None);
            device.destroy_pipeline(self.graphics_pipeline, None);
        }
    }
}

/// Create the pipeline which converts a buffer of vertices or indices to a framebuffer
pub fn create_pipeline(device: &ash::Device, swapchain_settings: SwapchainSettings) -> Result<Pipeline>
{
    let render_pass = create_render_pass(device, swapchain_settings)?;
    let descriptor_set_layout = create_descriptor_set_layout(device)?;
    let pipeline_layout = create_pipeline_layout(device, descriptor_set_layout)?;
    let vertex_shader_module = create_shader_module(device, String::from("vertexshader.spv"))?;
    let fragment_shader_module = create_shader_module(device, String::from("fragmentshader.spv"))?;
    let graphics_pipeline = create_graphics_pipeline(
        device,
        swapchain_settings,
        pipeline_layout,
        render_pass,
        vertex_shader_module,
        fragment_shader_module,
    )?;

    Ok(Pipeline {
        render_pass,
        descriptor_set_layout,
        pipeline_layout,
        graphics_pipeline,
    })
}

/// The render pass specifies details about the framebuffer attachments that are used while rendering
fn create_render_pass(device: &ash::Device, swapchain_settings: SwapchainSettings) -> Result<vk::RenderPass>
{
    // We have just one attachment, a colour buffer attachment represented by one of the images from the swapchain
    let colour_attachment = vk::AttachmentDescription::default()
        .format(swapchain_settings.format.format)
        .samples(vk::SampleCountFlags::TYPE_1) // No multisampling
        .load_op(vk::AttachmentLoadOp::CLEAR) // Clear the values to a constant at start of render
        .store_op(vk::AttachmentStoreOp::STORE) // Store rendered contents in memory after rendering that can be read later
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED) // Layout the image has before render pass begins, we don't care what previous layout the image was in
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR); // Layout to transition to when render pass ends, we want to present the image after rendering

    /*  A render pass can have multiple subpasses
        A subpass is a rendering operation that depends on the contents of framebuffers in previous passes e.g for a sequence of post-processing effects
        We only have one subpass
        Each subpass references one or more of our defined attachments using vk::AttachmentReference structs
    */
    // We get a reference to our colour attachment
    let colour_attachment_ref = vk::AttachmentReference::default()
        .attachment(0) // Which attachment to reference from all our AttachmentDescriptions
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL); // The attachment functions as a colour buffer, this layout gives the best performance
    let colour_attachments = [colour_attachment_ref];

    // Describe our only subpass
    let subpass = vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        // The index of this attachment in the array is directly referenced in the fragment shader with layout(location = 0) out vec4 outColour
        .color_attachments(&colour_attachments); // Make sure we include our reference to the attachment

    /*  Subpasses in a render pass automatically take care of image layout transitions
        These transitions are controlled by subpass dependencies
        They specify memory and execution dependencies between subpasses
    */
    let subpass_dependency = vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL) // Refers to implicit subpass before the render pass
        .dst_subpass(0) // Our only subpass index
        // Wait for swapchain to finish reading from image before we access it
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::empty())
        // Colour attachment stage write should wait on this
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);

    let attachments = [colour_attachment];
    let subpasses = [subpass];
    let dependencies = [subpass_dependency];
    // Now create the render pass
    let render_pass_create_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);

    Ok(unsafe { device.create_render_pass(&render_pass_create_info, None) }?)
}

/// The descriptor layout specifies the types of resources that are going to be accessed by the pipeline, for example uniform buffers and images
fn create_descriptor_set_layout(device: &ash::Device) -> Result<vk::DescriptorSetLayout>
{
    let ubo_layout_binding = vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::VERTEX);

    let sampler_layout_binding = vk::DescriptorSetLayoutBinding::default()
        .binding(1)
        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::FRAGMENT);

    let bindings = [ubo_layout_binding, sampler_layout_binding];
    let layout_create_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);

    Ok(unsafe { device.create_descriptor_set_layout(&layout_create_info, None)? })
}

/// The pipeline layout specifies uniform values in shaders and push constants (another way of passing dynamic values to shaders)
fn create_pipeline_layout(device: &ash::Device, descriptor_set_layout: vk::DescriptorSetLayout)
    -> Result<vk::PipelineLayout>
{
    let layouts = [descriptor_set_layout];
    let pipeline_layout_create_info = vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts);

    Ok(unsafe { device.create_pipeline_layout(&pipeline_layout_create_info, None) }?)
}

/// Create a shader module from a file containing valid SPIR-V bytecode
pub fn create_shader_module(device: &ash::Device, path: String) -> Result<vk::ShaderModule>
{
    let mut file = std::fs::File::open(&path).to_result(path.as_str())?;

    let code = ash::util::read_spv(&mut file).to_result(path.as_str())?;

    let shader_module_create_info = vk::ShaderModuleCreateInfo::default().code(code.as_slice());
    Ok(unsafe { device.create_shader_module(&shader_module_create_info, None) }?)
}

/// The graphics pipeline is the final result of combining the pipeline structures
///
/// Shader modules: Define functionality of programmable stages of graphics pipeline
///
/// Fixed-function state: Structures that define fixed-function stages e.g. input assembly, rasterizer, colour blending (created in this function)
///
/// Pipeline layout: Uniform and push values referenced by shader that can be updated at draw time
///
/// Render pass: Attachments referenced by the pipeline stages and their usage
fn create_graphics_pipeline(
    device: &ash::Device, swapchain_settings: SwapchainSettings, pipeline_layout: vk::PipelineLayout,
    render_pass: vk::RenderPass, vertex_shader_module: vk::ShaderModule, fragment_shader_module: vk::ShaderModule,
) -> Result<vk::Pipeline>
{
    /*  Initialize dynamic state information for the viewport and scissor
        Allows us to modify viewport and scissor during runtime without having to reconstruct the pipeline
    */
    let dynamic_states: [vk::DynamicState; 2] = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];

    let dynamic_state_create_info = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

    // The region of the framebuffer that the output is rendered to
    let viewport = vk::Viewport::default()
        .x(0.0)
        .y(0.0)
        .width(swapchain_settings.extent.width as f32)
        .height(swapchain_settings.extent.height as f32)
        .min_depth(0.0)
        .max_depth(1.0);

    /*  The scissor rectangle defines which regions pixels are stored
        Pixels outside the scissor are discarded by the rasterizer
    */
    let scissor = vk::Rect2D::default()
        .offset(vk::Offset2D { x: 0, y: 0 })
        .extent(swapchain_settings.extent);

    let viewports = [viewport];
    let scissors = [scissor];
    // We only need to tell Vulkan how many viewports and scissors we have - they are setup at draw time
    let viewport_state_create_info = vk::PipelineViewportStateCreateInfo::default()
        .viewports(&viewports)
        .scissors(&scissors);

    // Setup the fixed-function rasterizer
    // The rasterizer turns the output geometry from the vertex shader into fragments
    let rasterizer_create_info = vk::PipelineRasterizationStateCreateInfo::default()
        .depth_clamp_enable(false) // Discard fragments beyond the near and far planes
        .depth_bias_enable(false)
        .depth_bias_constant_factor(0.0)
        .depth_bias_clamp(0.0)
        .depth_bias_slope_factor(0.0)
        .rasterizer_discard_enable(false) // Enable rasterizer
        .polygon_mode(vk::PolygonMode::FILL)
        .line_width(1.0)
        .cull_mode(vk::CullModeFlags::BACK)
        .front_face(vk::FrontFace::CLOCKWISE); // Specify vertex order for faces

    // Setup multisampling (used for anti-aliasing) - currently disabled
    let multisampling_create_info = vk::PipelineMultisampleStateCreateInfo::default()
        .sample_shading_enable(false)
        .rasterization_samples(vk::SampleCountFlags::TYPE_1)
        .min_sample_shading(1.0)
        .alpha_to_coverage_enable(false)
        .alpha_to_one_enable(false);

    // Depth/stencil buffer here

    /*  Setup fixed-function colour blending
        Combines output fragment shader colour with framebuffer colour
        Can mix old and new value to produce final colour
        Can combine old and new value using bitwise operation
        Can overwrite framebuffer colour value with fragment shader value (i.e disabled) <- Current
    */

    // Configuration for colour blending per attached framebuffer (we currently only have one framebuffer)
    let colour_blend_attachment = vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(
            vk::ColorComponentFlags::R
                | vk::ColorComponentFlags::G
                | vk::ColorComponentFlags::B
                | vk::ColorComponentFlags::A,
        )
        .blend_enable(false)
        .src_color_blend_factor(vk::BlendFactor::ONE)
        .dst_color_blend_factor(vk::BlendFactor::ZERO)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
        .alpha_blend_op(vk::BlendOp::ADD);

    let attachments = [colour_blend_attachment];
    // Global colour blend setings
    let colour_blend_create_info = vk::PipelineColorBlendStateCreateInfo::default()
        .logic_op_enable(false)
        .attachments(&attachments);

    // How to pass vertex information to GPU memory
    let binding_description = vk::VertexInputBindingDescription::default()
        .binding(0)
        .stride(size_of::<vk_app::Vertex>() as u32)
        .input_rate(vk::VertexInputRate::VERTEX);

    // Describe how to extract a vetex attribute from our vertices array
    let attribute_descriptions: [vk::VertexInputAttributeDescription; 3] = [
        vk::VertexInputAttributeDescription::default()
            .binding(0)
            .location(0)
            .format(vk::Format::R32G32B32_SFLOAT) // vec3
            .offset(offset_of!(vk_app::Vertex, position) as u32),
        vk::VertexInputAttributeDescription::default()
            .binding(0)
            .location(1)
            .format(vk::Format::R32G32B32_SFLOAT) // vec3
            .offset(offset_of!(vk_app::Vertex, colour) as u32),
        vk::VertexInputAttributeDescription::default()
            .binding(0)
            .location(2)
            .format(vk::Format::R32G32_SFLOAT) // vec2
            .offset(offset_of!(vk_app::Vertex, tex_coord) as u32),
    ];

    let binding_descriptions = [binding_description];
    // Tell the pipeline about how we pass vertex information to the GPU
    let vertex_input_create_info = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&binding_descriptions)
        .vertex_attribute_descriptions(&attribute_descriptions);

    let input_assembly_create_info = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
        .primitive_restart_enable(false);

    let vertex_shader_stage_create_info = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::VERTEX)
        .module(vertex_shader_module)
        .name(&c"main");

    let fragment_shader_stage_create_info = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::FRAGMENT)
        .module(fragment_shader_module)
        .name(&c"main");

    let shader_stages = [vertex_shader_stage_create_info, fragment_shader_stage_create_info];

    let graphics_pipeline_create_info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&shader_stages)
        .vertex_input_state(&vertex_input_create_info)
        .input_assembly_state(&input_assembly_create_info)
        .viewport_state(&viewport_state_create_info)
        .rasterization_state(&rasterizer_create_info)
        .multisample_state(&multisampling_create_info)
        .color_blend_state(&colour_blend_create_info)
        .dynamic_state(&dynamic_state_create_info)
        .layout(pipeline_layout)
        .render_pass(render_pass)
        .subpass(0) // Index
        .base_pipeline_handle(vk::Pipeline::null()) // No parent pipeline
        .base_pipeline_index(-1);

    let create_infos = [graphics_pipeline_create_info];
    let graphics_pipelines = unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &create_infos, None) }
        .map_err(|errors| errors.1)?;

    unsafe {
        device.destroy_shader_module(vertex_shader_module, None);
        device.destroy_shader_module(fragment_shader_module, None)
    };

    Ok(graphics_pipelines
        .first()
        .expect("Error getting first pipeline - should not happen!")
        .to_owned())
}
